use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::board::Row;
use crate::cli::{LsArgs, WatchArgs};
use crate::error::{Error, Result};
use crate::state::{Lock, Paths};
use crate::{interrupted, output, rex, serve, stop_on_signals};

const START_WITHIN: Duration = Duration::from_secs(2);
/// serve.log is cut back to nothing past this size when a client starts serve.
const LOG_MAX: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Rows {
    rows: Vec<Row>,
    hosts: BTreeMap<String, String>,
}

fn connect(paths: &Paths) -> std::io::Result<UnixStream> {
    let _guard = rex::fd_guard();
    UnixStream::connect(&paths.socket)
}

/// Connects to serve, starting it first when nobody answers.
pub fn connect_or_start(paths: &Paths) -> Result<UnixStream> {
    if let Ok(stream) = connect(paths) {
        return Ok(stream);
    }
    paths.create()?;
    if paths.socket.exists() && !Lock::held(&paths.lock()) {
        let _ = std::fs::remove_file(&paths.socket);
    }
    start(paths)?;
    let deadline = Instant::now() + START_WITHIN;
    loop {
        match connect(paths) {
            Ok(stream) => return Ok(stream),
            Err(err) if Instant::now() >= deadline => {
                return Err(Error::Serve(format!(
                    "serve did not answer on {} within {}s ({err}); see {}",
                    paths.socket.display(),
                    START_WITHIN.as_secs(),
                    paths.log().display()
                )));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Starts `rexboard serve --foreground` in its own session, with its output in serve.log.
fn start(paths: &Paths) -> Result<u32> {
    let log_path = paths.log();
    if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > LOG_MAX) {
        let _ = std::fs::remove_file(&log_path);
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(["serve", "--foreground"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = rex::spawn(&mut cmd)
        .map_err(|err| Error::Serve(format!("could not start serve: {err}")))?;
    Ok(child.id())
}

fn send(stream: &mut UnixStream, request: &Value) -> Result<()> {
    writeln!(stream, "{request}").map_err(|err| Error::Serve(format!("serve went away: {err}")))
}

/// Reads one reply line. `line` carries a partial line across a read timeout.
fn read(reader: &mut impl BufRead, line: &mut String) -> Result<Option<Value>> {
    match reader.read_line(line) {
        Ok(0) => Ok(None),
        Ok(_) if !line.ends_with('\n') => Err(Error::Io(std::io::ErrorKind::WouldBlock.into())),
        Ok(_) => {
            let value = parse(line);
            line.clear();
            value.map(Some)
        }
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Err(Error::Io(err))
        }
        Err(err) => Err(Error::Serve(format!("serve went away: {err}"))),
    }
}

fn parse(line: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Serve(format!("serve sent a line that is not JSON: {err}")))?;
    if value["v"] != serve::VERSION {
        return Err(Error::ContractMismatch(format!(
            "serve speaks protocol version {}, this rexboard speaks {}",
            value["v"],
            serve::VERSION
        )));
    }
    if let Some(error) = value.get("error") {
        let message = error["message"].as_str().unwrap_or_default().to_owned();
        return Err(match error["code"].as_str() {
            Some("NOT_FOUND" | "NOBODY_NEEDS_YOU") => Error::NotFound(message),
            Some("USAGE") => Error::usage_with(message, &["rexboard --help"]),
            code => Error::Serve(format!("{}: {message}", code.unwrap_or("ERROR"))),
        });
    }
    Ok(value)
}

/// One request, one reply.
pub fn call(request: &Value) -> Result<Value> {
    let paths = Paths::find()?;
    let mut stream = connect_or_start(&paths)?;
    send(&mut stream, request)?;
    read(&mut BufReader::new(stream), &mut String::new())?
        .ok_or_else(|| Error::Serve("serve closed the connection without answering".into()))
}

fn rows(value: Value) -> Result<Rows> {
    serde_json::from_value(value)
        .map_err(|err| Error::Serve(format!("serve sent rows in an unexpected shape: {err}")))
}

/// Keeps the rows `ls` or `watch` asked for, and checks the host exists.
fn pick(mut rows: Rows, host: Option<&str>, all: bool) -> Result<Vec<Row>> {
    if let Some(want) = host {
        if !rows.hosts.contains_key(want) {
            return Err(Error::usage_with(
                format!("no host named {want}"),
                &["rex hosts", "rexboard ls"],
            ));
        }
        rows.hosts.retain(|h, _| h == want);
        rows.rows.retain(|row| row.pane.host == want);
    }
    Ok(rows
        .rows
        .into_iter()
        .filter(|row| all || row.pane.is_agent())
        .collect())
}

pub fn ls(args: &LsArgs, json: bool) -> Result<()> {
    let rows = rows(call(&json!({ "cmd": "rows" }))?)?;
    let considered: Vec<_> = rows
        .hosts
        .iter()
        .filter(|(h, _)| args.host.as_deref().is_none_or(|want| want == h.as_str()))
        .map(|(h, s)| (h.clone(), s.clone()))
        .collect();
    let list = pick(rows, args.host.as_deref(), args.all)?;
    let down: Vec<_> = considered.iter().filter(|(_, s)| s != "up").collect();
    if !considered.is_empty() && down.len() == considered.len() {
        let names: Vec<_> = down.iter().map(|(h, s)| format!("{h} ({s})")).collect();
        return Err(Error::HostsUnreachable(format!(
            "every host is unreachable: {}",
            names.join(", ")
        )));
    }
    for (host, state) in down {
        eprintln!("warning: {host} is unreachable ({state})");
    }
    if json {
        println!(
            "{}",
            serde_json::to_string(&list).map_err(std::io::Error::other)?
        );
    } else {
        print!("{}", output::table(&list, chrono::Utc::now()));
    }
    Ok(())
}

/// Prints the agent rows on every change, until --for, a signal, or serve stopping.
pub fn watch(args: &WatchArgs) -> Result<()> {
    let paths = Paths::find()?;
    let mut stream = connect_or_start(&paths)?;
    send(&mut stream, &json!({ "cmd": "subscribe" }))?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stop_on_signals();
    let deadline = args
        .duration
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut out = std::io::stdout().lock();
    while !interrupted() && deadline.is_none_or(|d| Instant::now() < d) {
        let value = match read(&mut reader, &mut line) {
            Ok(Some(value)) => value,
            Ok(None) => return Err(Error::Serve("serve stopped".into())),
            Err(Error::Io(_)) => continue,
            Err(err) => return Err(err),
        };
        if value.get("rows").is_none() {
            continue;
        }
        let list = pick(rows(value)?, args.host.as_deref(), false)?;
        let text = serde_json::to_string(&list).map_err(std::io::Error::other)?;
        if writeln!(out, "{text}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
    Ok(())
}

pub fn seen(key: &str, json: bool) -> Result<()> {
    call(&json!({ "cmd": "seen", "key": key }))?;
    if json {
        println!("{}", json!({ "ok": true, "key": key }));
    } else {
        println!("seen {key}");
    }
    Ok(())
}

/// `serve` without `--foreground`: start it in the background unless one already answers.
pub fn start_serve(json: bool) -> Result<()> {
    let paths = Paths::find()?;
    let running = connect(&paths).is_ok();
    if !running {
        drop(connect_or_start(&paths)?);
    }
    let pid = std::fs::read_to_string(paths.lock())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|record| record["pid"].as_i64());
    let pid = pid.map_or("unknown".to_owned(), |p| p.to_string());
    if json {
        println!(
            "{}",
            json!({ "ok": true, "pid": pid, "already_running": running, "socket": paths.socket })
        );
    } else if running {
        println!("serve is already running (pid {pid})");
    } else {
        println!("serve started (pid {pid}), log {}", paths.log().display());
    }
    Ok(())
}

pub fn stop_serve(json: bool) -> Result<()> {
    let paths = Paths::find()?;
    let Ok(mut stream) = connect(&paths) else {
        if json {
            println!("{}", json!({ "ok": true, "running": false }));
        } else {
            println!("serve is not running");
        }
        return Ok(());
    };
    send(&mut stream, &json!({ "cmd": "stop" }))?;
    read(&mut BufReader::new(stream), &mut String::new())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Lock::held(&paths.lock()) {
        if Instant::now() >= deadline {
            return Err(Error::Serve(format!(
                "serve did not stop within 5s; see {}",
                paths.log().display()
            )));
        }
        thread::sleep(Duration::from_millis(20));
    }
    if json {
        println!("{}", json!({ "ok": true, "running": false }));
    } else {
        println!("serve stopped");
    }
    Ok(())
}
