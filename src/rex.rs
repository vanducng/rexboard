use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(10);

pub const SNAPSHOT: &str = concat!(
    include_str!("lua/lib.lua"),
    "\n",
    include_str!("lua/snapshot.lua")
);

/// Reads the end of one pane; the args are `session_id`, `block_id` and `max_bytes`.
pub const TAIL: &str = concat!(include_str!("lua/tail.lua"), "\nreturn tail(rex.args)");

static FDS: Mutex<()> = Mutex::new(());

/// macOS std creates pipes and sockets, then sets close-on-exec in a second step, so a child
/// spawned on another thread in between inherits them. Hold this around every spawn, bind,
/// accept and connect.
pub fn fd_guard() -> MutexGuard<'static, ()> {
    FDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A `rex` command. rexboard names its own targets, so a caller's (maybe a pane on another host)
/// must not reach rex as the default block, session or app client.
pub fn command(rex: &Path) -> Command {
    let mut cmd = Command::new(rex);
    for var in ["REX_BLOCK", "REX_SESSION", "REX_CLIENT"] {
        cmd.env_remove(var);
    }
    cmd
}

pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let _guard = fd_guard();
    cmd.spawn()
}

pub fn locate() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("REXBOARD_REX") {
        return Ok(path.into());
    }
    if let Some(dir) = std::env::var_os("REX_BIN_DIR") {
        let path = PathBuf::from(dir).join("rex");
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(path) = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join("rex"))
        .find(|path| path.is_file())
    {
        return Ok(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(
            "Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex",
        );
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(Error::RexNotFound(
        "no rex CLI in REXBOARD_REX, REX_BIN_DIR, PATH, or the Rex app bundle".into(),
    ))
}

/// Runs `rex [-S host] do - --json --args <json>` with `lua` on stdin and returns its JSON result.
pub fn run_script(host: Option<&str>, lua: &str, args: &Value) -> Result<Value> {
    run_script_with(&locate()?, host, lua, args, TIMEOUT, &|| false).map_err(|err| match err {
        Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Error::Rex(format!("rex did not answer within {}s", TIMEOUT.as_secs()))
        }
        err => err,
    })
}

/// `rex [-S host] do -` with `lua`, killed after `limit` (an io `TimedOut` error) or as soon as
/// `cancelled` says so (`Error::Cancelled`).
pub fn run_script_with(
    rex: &Path,
    host: Option<&str>,
    lua: &str,
    args: &Value,
    limit: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let mut cmd = command(rex);
    if let Some(host) = host {
        cmd.args(["-S", host]);
    }
    cmd.args(["do", "-", "--json", "--args", &args.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(&mut cmd)
        .map_err(|err| Error::RexNotFound(format!("could not run {}: {err}", rex.display())))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let lua = lua.to_owned();
    let writer = thread::spawn(move || stdin.write_all(lua.as_bytes()));
    let stdout = drain(child.stdout.take().expect("piped stdout"));
    let stderr = drain(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let gave_up = Instant::now() >= deadline;
        if gave_up || cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if gave_up {
                std::io::Error::from(std::io::ErrorKind::TimedOut).into()
            } else {
                Error::Cancelled
            });
        }
        thread::sleep(Duration::from_millis(5));
    };
    let _ = writer.join();
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        let why = stderr.lines().find(|line| !line.trim().is_empty());
        return Err(Error::Rex(format!(
            "rex do failed ({status}): {}",
            why.unwrap_or("no output")
        )));
    }
    serde_json::from_str(&stdout)
        .map_err(|err| Error::Rex(format!("rex do printed something that is not JSON: {err}")))
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}
