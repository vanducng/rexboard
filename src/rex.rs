use std::io::{Read, Write};
use std::path::PathBuf;
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

static FDS: Mutex<()> = Mutex::new(());

/// macOS std creates pipes and sockets, then sets close-on-exec in a second step, so a child
/// spawned on another thread in between inherits them. Hold this around every spawn, bind,
/// accept and connect.
pub fn fd_guard() -> MutexGuard<'static, ()> {
    FDS.lock().unwrap_or_else(PoisonError::into_inner)
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
    let rex = locate()?;
    let mut cmd = Command::new(&rex);
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
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Rex(format!(
                "rex did not answer within {}s",
                TIMEOUT.as_secs()
            )));
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
