use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::board::{Memory, Transition};
use crate::error::{Error, Result};
use crate::rex;

/// `sun_path` is 104 bytes on macOS, including the closing NUL.
const SOCKET_MAX: usize = 103;
/// transitions.jsonl rolls over to transitions.jsonl.1 past this size when serve starts.
const TRANSITIONS_MAX: u64 = 5 * 1024 * 1024;

pub struct Paths {
    pub socket: PathBuf,
    pub dir: PathBuf,
}

impl Paths {
    /// The socket sits beside serve.lock, so every caller finds the one serve whatever its
    /// TMPDIR. XDG_RUNTIME_DIR, when set and short enough, wins.
    pub fn find() -> Result<Self> {
        let dir = match std::env::var_os("XDG_STATE_HOME") {
            Some(dir) => PathBuf::from(dir).join("rexboard"),
            None => {
                let home = std::env::var_os("HOME")
                    .ok_or_else(|| Error::serve("neither XDG_STATE_HOME nor HOME is set"))?;
                PathBuf::from(home).join(".local/state/rexboard")
            }
        };
        let socket = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|run| PathBuf::from(run).join("rexboard.sock"))
            .filter(|path| path.as_os_str().len() <= SOCKET_MAX)
            .unwrap_or_else(|| dir.join("serve.sock"));
        if socket.as_os_str().len() > SOCKET_MAX {
            return Err(Error::serve_with(
                format!(
                    "socket path {} is longer than {SOCKET_MAX} bytes; set XDG_RUNTIME_DIR to a shorter directory",
                    socket.display()
                ),
                &["export XDG_RUNTIME_DIR=/tmp", "rexboard ls --direct"],
            ));
        }
        Ok(Self { socket, dir })
    }

    pub fn create(&self) -> Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(|err| Error::serve(format!("could not create {}: {err}", self.dir.display())))
    }

    pub fn state(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn transitions(&self) -> PathBuf {
        self.dir.join("transitions.jsonl")
    }

    pub fn log(&self) -> PathBuf {
        self.dir.join("serve.log")
    }

    pub fn lock(&self) -> PathBuf {
        self.dir.join("serve.lock")
    }
}

/// What serve.lock records: the serve and the observers it started.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Record {
    pub pid: i32,
    #[serde(default)]
    pub rex: String,
    #[serde(default)]
    pub children: Vec<i32>,
}

/// The `flock` on serve.lock. Held for the life of serve; the kernel drops it if serve dies.
pub struct Lock {
    file: File,
}

impl Lock {
    /// Takes the lock, or returns None when another serve holds it. Also returns what the last
    /// holder recorded, for the startup sweep.
    pub fn take(path: &Path) -> Result<Option<(Self, Record)>> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        // A client's held() probe takes the lock for an instant, so a busy lock gets a few tries.
        let mut tries = 0;
        while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EWOULDBLOCK) {
                return Err(err.into());
            }
            tries += 1;
            if tries == 5 {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        let previous = serde_json::from_str(&text).unwrap_or_default();
        Ok(Some((Self { file }, previous)))
    }

    /// Whether a running serve holds the lock. A probe that gets it lets go at once.
    pub fn held(path: &Path) -> bool {
        let Ok(file) = File::open(path) else {
            return false;
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
        rc != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
    }

    /// One write at offset 0, then the length, so a reader never sees an empty file.
    pub fn write(&mut self, record: &Record) -> Result<()> {
        let body = serde_json::to_vec(record).map_err(std::io::Error::other)?;
        self.file.rewind()?;
        self.file.write_all(&body)?;
        self.file.set_len(body.len() as u64)?;
        Ok(())
    }
}

/// Stops observers a dead serve left behind. A recorded pid may have been reused, so each one
/// must still be a group leader running `rex ... do - --all --fail-fast`, orphaned or still a
/// child of the recorded serve.
pub fn sweep(previous: &Record) {
    for &pid in &previous.children {
        match observer(pid, previous) {
            Ok(()) => {
                unsafe { libc::kill(-pid, libc::SIGTERM) };
                eprintln!(
                    "sweep: stopped observer {pid} left by serve {}",
                    previous.pid
                );
            }
            Err(why) => eprintln!("sweep: skipped pid {pid}: {why}"),
        }
    }
}

fn observer(pid: i32, previous: &Record) -> std::result::Result<(), String> {
    let mut cmd = Command::new("/bin/ps");
    cmd.args(["-o", "ppid=,pgid=,command=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = rex::spawn(&mut cmd)
        .and_then(std::process::Child::wait_with_output)
        .map_err(|err| format!("ps failed: {err}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim();
    if line.is_empty() {
        return Err("no such process".into());
    }
    let mut fields = line.split_whitespace();
    let ppid: i32 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(-1);
    let pgid: i32 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(-1);
    let command = fields.collect::<Vec<_>>().join(" ");
    if pgid != pid {
        return Err(format!("not its own process group (pgid {pgid})"));
    }
    if ppid != 1 && ppid != previous.pid {
        return Err(format!(
            "parent {ppid} is neither init nor serve {}",
            previous.pid
        ));
    }
    if previous.rex.is_empty() || !command.starts_with(&previous.rex) {
        return Err(format!("command is not the recorded rex CLI: {command}"));
    }
    if !command.ends_with("do - --all --fail-fast") {
        return Err(format!("command is not an observer: {command}"));
    }
    Ok(())
}

pub fn load(path: &Path) -> BTreeMap<String, Memory> {
    #[derive(Deserialize)]
    struct Saved {
        panes: BTreeMap<String, Memory>,
    }
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Saved>(&text) {
            Ok(saved) => saved.panes,
            Err(err) => {
                eprintln!("warning: ignoring {}: {err}", path.display());
                BTreeMap::new()
            }
        },
        Err(_) => BTreeMap::new(),
    }
}

/// Writes state.json through a temp file and a rename, so a crash never leaves half a file.
pub fn save(path: &Path, panes: &BTreeMap<String, Memory>) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let body = json!({ "v": 1, "panes": panes });
    fs::write(&tmp, body.to_string())?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn append(path: &Path, transitions: &[Transition]) -> Result<()> {
    if transitions.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut text = String::new();
    for transition in transitions {
        text.push_str(&serde_json::to_string(transition).map_err(std::io::Error::other)?);
        text.push('\n');
    }
    file.write_all(text.as_bytes())?;
    Ok(())
}

/// Keeps transitions.jsonl bounded: one older file, then it is gone.
pub fn roll(path: &Path) {
    if fs::metadata(path).is_ok_and(|m| m.len() > TRANSITIONS_MAX) {
        let _ = fs::rename(path, path.with_extension("jsonl.1"));
    }
}
