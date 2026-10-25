use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
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

#[derive(Debug, Default, Deserialize)]
pub struct Rows {
    pub rows: Vec<Row>,
    pub hosts: BTreeMap<String, String>,
}

/// None when nothing answers. A socket served by another user is an error, since `/tmp` is shared.
fn connect(paths: &Paths) -> Result<Option<UnixStream>> {
    let stream = {
        let _guard = rex::fd_guard();
        UnixStream::connect(&paths.socket)
    };
    let Ok(stream) = stream else {
        return Ok(None);
    };
    let me = unsafe { libc::getuid() };
    match peer_uid(&stream) {
        Ok(uid) if uid == me => Ok(Some(stream)),
        Ok(uid) => Err(Error::serve_with(
            format!(
                "{} is served by uid {uid}, not you (uid {me})",
                paths.socket.display()
            ),
            &["unset XDG_STATE_HOME", "rexboard ls --direct"],
        )),
        Err(err) => Err(Error::serve(format!(
            "could not check who serves {}: {err}",
            paths.socket.display()
        ))),
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> std::io::Result<libc::uid_t> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(cred.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(stream: &UnixStream) -> std::io::Result<libc::uid_t> {
    let (mut uid, mut gid) = (0, 0);
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(uid)
}

/// Connects to serve, starting it first when nobody answers and no live serve holds the lock.
/// serve itself clears a stale socket once it holds the lock, so the client never unlinks one.
pub fn connect_or_start(paths: &Paths) -> Result<UnixStream> {
    if let Some(stream) = connect(paths)? {
        return Ok(stream);
    }
    paths.create()?;
    // A serve that holds the lock may still be binding; it gets the same wait as one we start.
    let held = Lock::held(&paths.lock());
    if !held {
        start(paths)?;
    }
    let deadline = Instant::now() + START_WITHIN;
    loop {
        if let Some(stream) = connect(paths)? {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            // Checked again, so a serve that exited during the wait is not named.
            if held && Lock::held(&paths.lock()) {
                let pid = recorded_pid(paths).map_or("?".to_owned(), |p| p.to_string());
                return Err(Error::serve_with(
                    format!(
                        "serve (pid {pid}) holds {} but does not answer on {}. It is probably from an older rexboard, or it is stuck. Stop it; the next command starts a new one",
                        paths.lock().display(),
                        paths.socket.display()
                    ),
                    &["rexboard serve --stop", "rexboard ls"],
                ));
            }
            return Err(Error::serve(format!(
                "serve did not answer on {} within {}s; see {}",
                paths.socket.display(),
                START_WITHIN.as_secs(),
                paths.log().display()
            )));
        }
        thread::sleep(Duration::from_millis(20));
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
    let mut child = rex::spawn(&mut cmd)
        .map_err(|err| Error::serve(format!("could not start serve: {err}")))?;
    let pid = child.id();
    // Reaped here so a long-running client never keeps a dead serve as a zombie.
    thread::spawn(move || child.wait());
    Ok(pid)
}

fn send(stream: &mut UnixStream, request: &Value) -> Result<()> {
    writeln!(stream, "{request}").map_err(|err| Error::serve(format!("serve went away: {err}")))
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
        Err(err) => Err(Error::serve(format!("serve went away: {err}"))),
    }
}

fn parse(line: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::serve(format!("serve sent a line that is not JSON: {err}")))?;
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
            Some("NOT_FOUND") => Error::NotFound(message),
            Some("NOBODY_NEEDS_YOU") => Error::NobodyNeedsYou,
            Some("USAGE") => Error::usage_with(message, &["rexboard --help"]),
            code => Error::serve(format!("{}: {message}", code.unwrap_or("ERROR"))),
        });
    }
    Ok(value)
}

/// One request, one reply. A serve that closes without answering, as one that is stopping
/// does, gets one more try, which starts a new serve if needed.
pub fn call(request: &Value) -> Result<Value> {
    let paths = Paths::find()?;
    for _ in 0..2 {
        let mut stream = connect_or_start(&paths)?;
        send(&mut stream, request)?;
        if let Some(reply) = read(&mut BufReader::new(stream), &mut String::new())? {
            return Ok(reply);
        }
    }
    Err(Error::serve(
        "serve closed the connection without answering, twice",
    ))
}

fn rows(value: Value) -> Result<Rows> {
    serde_json::from_value(value)
        .map_err(|err| Error::serve(format!("serve sent rows in an unexpected shape: {err}")))
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
    let log = Paths::find()?.log();
    let see = if down.iter().any(|(_, s)| s == "down" || s == "stopped") {
        format!("; see {}", log.display())
    } else {
        String::new()
    };
    if !considered.is_empty() && down.len() == considered.len() {
        let names: Vec<_> = down.iter().map(|(h, s)| format!("{h} ({s})")).collect();
        return Err(Error::HostsUnreachable(format!(
            "every host is unreachable: {}{see}",
            names.join(", ")
        )));
    }
    for (host, state) in down {
        match state.as_str() {
            "starting" => eprintln!("warning: {host} has not sent its first snapshot yet"),
            "down" => eprintln!("warning: {host} is down, retrying; its rows may be stale{see}"),
            _ => eprintln!(
                "warning: {host} stopped and will not restart; see {}",
                log.display()
            ),
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string(&list).map_err(std::io::Error::other)?
        );
    } else {
        print!(
            "{}",
            output::table(&list, chrono::Utc::now(), output::terminal_width())
        );
    }
    Ok(())
}

/// Prints the agent rows on every change, until --for, a signal, or serve stopping.
/// Calls `each` with every rows line serve sends, and with None at least every 200ms, until
/// serve closes or `each` returns false.
pub fn subscribe(mut each: impl FnMut(Option<Rows>) -> bool) -> Result<()> {
    let paths = Paths::find()?;
    let mut stream = connect_or_start(&paths)?;
    send(&mut stream, &json!({ "cmd": "subscribe" }))?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        let next = match read(&mut reader, &mut line) {
            Ok(Some(value)) if value.get("rows").is_some() => Some(rows(value)?),
            Ok(Some(_)) | Err(Error::Io(_)) => None,
            Ok(None) => return Err(Error::serve("serve stopped")),
            Err(err) => return Err(err),
        };
        if !each(next) {
            return Ok(());
        }
    }
}

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
            Ok(None) => {
                return Err(Error::serve_with(
                    "serve stopped",
                    &["rexboard watch --json", "rexboard serve"],
                ));
            }
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
    if !key.contains('/') {
        return Err(Error::usage_with(
            format!("KEY must be HOST/BLOCK_ID, as in ls --json: {key}"),
            &["rexboard ls --json"],
        ));
    }
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
    let running = connect(&paths)?.is_some();
    if !running {
        drop(connect_or_start(&paths)?);
    }
    let pid = recorded_pid(&paths);
    if json {
        println!(
            "{}",
            json!({ "ok": true, "pid": pid, "already_running": running, "socket": paths.socket })
        );
    } else {
        let pid = pid.map_or("unknown".to_owned(), |p| p.to_string());
        if running {
            println!("serve is already running (pid {pid})");
        } else {
            println!("serve started (pid {pid}), log {}", paths.log().display());
        }
    }
    Ok(())
}

fn recorded_pid(paths: &Paths) -> Option<i32> {
    let text = std::fs::read_to_string(paths.lock()).ok()?;
    let pid = serde_json::from_str::<Value>(&text).ok()?["pid"].as_i64()?;
    i32::try_from(pid).ok()
}

fn alive(pid: i32) -> bool {
    let ok = unsafe { libc::kill(pid, 0) } == 0;
    ok || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn stop_serve(json: bool) -> Result<()> {
    let paths = Paths::find()?;
    let lock = paths.lock().display().to_string();
    let pid = match connect(&paths)? {
        Some(mut stream) => {
            let pid = recorded_pid(&paths);
            send(&mut stream, &json!({ "cmd": "stop" }))?;
            // serve may close as it stops; the wait below is what proves it stopped.
            let _ = read(&mut BufReader::new(stream), &mut String::new());
            pid
        }
        // A serve that holds the lock but does not answer (an older build, or a stuck one).
        None if Lock::held(&paths.lock()) => {
            let lsof = format!("lsof {lock}");
            let Some(pid) = recorded_pid(&paths) else {
                return Err(Error::serve_with(
                    format!("a serve holds {lock} but recorded no pid"),
                    &[lsof.as_str(), "kill <the pid lsof shows>"],
                ));
            };
            // The record can lag the lock, so never signal a pid that is not rexboard.
            let Some(exe) = exe_path(pid) else {
                return Err(Error::serve_with(
                    format!("cannot read pid {pid}'s executable, so it was not signalled"),
                    &[lsof.as_str(), "kill <the pid lsof shows>"],
                ));
            };
            if !rexboard_exe(&exe) {
                return Err(Error::serve_with(
                    format!(
                        "serve.lock names pid {pid}, which is not rexboard; serve may still be starting, retry"
                    ),
                    &["rexboard serve --stop", lsof.as_str()],
                ));
            }
            unsafe { libc::kill(pid, libc::SIGTERM) };
            Some(pid)
        }
        None => {
            if json {
                println!("{}", json!({ "ok": true, "running": false }));
            } else {
                println!("serve is not running");
            }
            return Ok(());
        }
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let running = || match pid {
        Some(pid) => alive(pid),
        None => matches!(connect(&paths), Ok(Some(_))),
    };
    while running() {
        if Instant::now() >= deadline {
            let Some(pid) = pid else {
                return Err(Error::serve(format!(
                    "serve did not exit within 5s; see {}",
                    paths.log().display()
                )));
            };
            let kill = format!("kill -9 {pid}");
            return Err(Error::serve_with(
                format!(
                    "serve (pid {pid}) did not exit within 5s; see {}",
                    paths.log().display()
                ),
                &[kill.as_str(), "rexboard ls"],
            ));
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

/// Whether an executable path is rexboard's. Linux shows a binary replaced in place, as
/// install.sh does, as "rexboard (deleted)".
fn rexboard_exe(path: &std::path::Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "rexboard" || name == "rexboard (deleted)")
}

#[cfg(target_os = "macos")]
fn exe_path(pid: i32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(std::ffi::OsStr::from_bytes(&buf[..len]).into())
}

#[cfg(not(target_os = "macos"))]
fn exe_path(pid: i32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rexboard_binaries_are_stopped() {
        assert!(rexboard_exe(std::path::Path::new(
            "/usr/local/bin/rexboard"
        )));
        assert!(rexboard_exe(std::path::Path::new(
            "/home/u/.local/bin/rexboard (deleted)"
        )));
        assert!(!rexboard_exe(std::path::Path::new("/usr/bin/perl")));
        assert!(!rexboard_exe(std::path::Path::new("/usr/bin/rexboard-old")));
    }
}
