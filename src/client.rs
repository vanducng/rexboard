use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Set while the board is up, whose screen a stderr line would break; notices then go only to
/// serve.log.
pub static QUIET: AtomicBool = AtomicBool::new(false);

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
/// A serve older than this rexboard is replaced once, so an upgrade takes effect by itself.
/// serve itself clears a stale socket once it holds the lock, so the client never unlinks one.
pub fn connect_or_start(paths: &Paths) -> Result<UnixStream> {
    let stream = reach(paths)?;
    let Some(old) = older_serve(paths) else {
        return Ok(stream);
    };
    drop(stream);
    stop(paths)?;
    notice(
        paths,
        &format!("restarted serve {old} -> {}", serve::version()),
    );
    reach(paths)
}

fn reach(paths: &Paths) -> Result<UnixStream> {
    if let Some(stream) = connect(paths)? {
        return Ok(stream);
    }
    paths.create()?;
    // A serve that holds the lock may still be binding; it gets the same wait as one we start.
    let mut held = Lock::held(&paths.lock());
    if !held {
        start(paths)?;
    }
    let mut healed = false;
    let mut deadline = Instant::now() + START_WITHIN;
    loop {
        if let Some(stream) = connect(paths)? {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            // Checked again, so a serve that exited during the wait is not named.
            if held && Lock::held(&paths.lock()) {
                let pid = recorded_pid(paths);
                // A rexboard that holds the lock and never answers is an older build listening
                // elsewhere, or a stuck one: replace it, once.
                if let Some(pid) = pid.filter(|&pid| !healed && is_rexboard(pid)) {
                    healed = true;
                    held = false;
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    wait_gone(paths, Some(pid))?;
                    notice(
                        paths,
                        &format!(
                            "restarted serve (pid {pid} did not answer) -> {}",
                            serve::version()
                        ),
                    );
                    start(paths)?;
                    deadline = Instant::now() + START_WITHIN;
                    continue;
                }
                let pid = pid.map_or("?".to_owned(), |p| p.to_string());
                return Err(Error::serve_with(
                    format!(
                        "serve (pid {pid}) holds {} but does not answer on {}, and it is not rexboard, so it was left alone",
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

/// The version of the serve on `paths` when it is older than this rexboard. One from before the
/// handshake does not know the request; doubt (no answer, an odd version) leaves serve alone.
fn older_serve(paths: &Paths) -> Option<String> {
    let mut stream = connect(paths).ok()??;
    send(&mut stream, &json!({ "cmd": "version" })).ok()?;
    stream.set_read_timeout(Some(START_WITHIN)).ok()?;
    match read(&mut BufReader::new(stream), &mut String::new()) {
        Ok(Some(reply)) => {
            let theirs = reply["rexboard"].as_str()?;
            older(theirs, &serve::version()).then(|| theirs.to_owned())
        }
        Err(Error::Usage { .. }) => Some("(no version)".to_owned()),
        _ => None,
    }
}

/// Whether version `a` comes before `b`, by their dotted numbers; never for one that is not.
fn older(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    matches!((parts(a), parts(b)), (Some(a), Some(b)) if a < b)
}

/// One line in serve.log and, unless the board is up, on stderr.
fn notice(paths: &Paths, line: &str) {
    if let Ok(mut log) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log())
    {
        let _ = writeln!(log, "{line}");
    }
    if !QUIET.load(Ordering::Relaxed) {
        eprintln!("{line}");
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
    let stopped = stop(&Paths::find()?)?;
    if json {
        println!("{}", json!({ "ok": true, "running": false }));
    } else if stopped {
        println!("serve stopped");
    } else {
        println!("serve is not running");
    }
    Ok(())
}

/// Stops the serve on `paths`; false when none was running.
fn stop(paths: &Paths) -> Result<bool> {
    let lock = paths.lock().display().to_string();
    let pid = match connect(paths)? {
        Some(mut stream) => {
            let pid = recorded_pid(paths);
            send(&mut stream, &json!({ "cmd": "stop" }))?;
            // serve may close as it stops; the wait below is what proves it stopped.
            let _ = read(&mut BufReader::new(stream), &mut String::new());
            pid
        }
        // A serve that holds the lock but does not answer (an older build, or a stuck one).
        None if Lock::held(&paths.lock()) => {
            let lsof = format!("lsof {lock}");
            let Some(pid) = recorded_pid(paths) else {
                return Err(Error::serve_with(
                    format!("a serve holds {lock} but recorded no pid"),
                    &[lsof.as_str(), "kill <the pid lsof shows>"],
                ));
            };
            // The record can lag the lock, so never signal a pid that is not rexboard.
            if !is_rexboard(pid) {
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
        None => return Ok(false),
    };
    wait_gone(paths, pid)?;
    Ok(true)
}

/// Waits up to 5s for `pid` to exit, or with no pid for nothing to answer on the socket.
fn wait_gone(paths: &Paths, pid: Option<i32>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let running = || match pid {
        Some(pid) => alive(pid),
        None => matches!(connect(paths), Ok(Some(_))),
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
    Ok(())
}

/// Whether `pid` runs rexboard: by its executable's path, or by its name when the path cannot
/// be read, as on macOS once an upgrade has deleted the binary it runs.
fn is_rexboard(pid: i32) -> bool {
    rexboard_process(exe_path(pid).as_deref(), proc_name(pid).as_deref())
}

fn rexboard_process(exe: Option<&std::path::Path>, name: Option<&str>) -> bool {
    exe.map_or(name == Some("rexboard"), rexboard_exe)
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

/// The process's own name (p_comm), which outlives a deleted binary.
#[cfg(target_os = "macos")]
fn proc_name(pid: i32) -> Option<String> {
    let mut buf = [0u8; 64];
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn proc_name(pid: i32) -> Option<String> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(name.trim_end().to_owned())
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

    #[test]
    fn a_process_whose_path_cannot_be_read_counts_by_its_name() {
        let path = std::path::Path::new;
        assert!(
            rexboard_process(None, Some("rexboard")),
            "a deleted binary on macOS"
        );
        assert!(!rexboard_process(None, Some("perl")));
        assert!(!rexboard_process(None, None), "nothing to go on");
        assert!(
            !rexboard_process(Some(path("/usr/bin/perl")), Some("rexboard")),
            "a readable path wins over the name"
        );
        assert!(rexboard_process(
            Some(path("/opt/homebrew/bin/rexboard")),
            None
        ));
    }

    #[test]
    fn only_an_older_dotted_version_counts_as_older() {
        assert!(older("0.3.0", "0.3.1"));
        assert!(older("0.9.9", "0.10.0"), "numbers, not text");
        assert!(!older("0.3.1", "0.3.1"));
        assert!(!older("0.4.0", "0.3.1"), "a newer serve is kept");
        assert!(!older("0.3.1-rc1", "0.3.1"), "doubt leaves serve alone");
    }
}
