use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::event::{self, Event};
use crate::rex;

pub const OBSERVER: &str = concat!(
    include_str!("lua/lib.lua"),
    "\n",
    include_str!("lua/observer.lua")
);

/// The observer ticks every 30s, so three missed ticks means the host is gone.
const SILENCE: Duration = Duration::from_secs(90);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub type HostId = String;

#[derive(Debug)]
pub enum HostEvent {
    Event(Event),
    /// The observer stopped; a new one starts after `retry_in`.
    Down {
        reason: String,
        retry_in: Duration,
    },
    /// The stream cannot be read. The collector has stopped.
    Failed(Error),
}

/// Keeps one `rex [-S host] do - --all` observer alive for a host.
pub struct Collector {
    stop: Arc<AtomicBool>,
    pgid: Arc<AtomicI32>,
}

impl Collector {
    pub fn spawn(host: Option<String>, tx: Sender<(HostId, HostEvent)>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pgid = Arc::new(AtomicI32::new(0));
        let (stop2, pgid2) = (stop.clone(), pgid.clone());
        thread::spawn(move || supervise(host, &tx, &stop2, &pgid2));
        Self { stop, pgid }
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        kill_group(self.pgid.swap(0, Ordering::SeqCst));
    }
}

fn kill_group(pgid: i32) {
    if pgid > 0 {
        unsafe { libc::kill(-pgid, libc::SIGTERM) };
    }
}

fn supervise(
    host: Option<String>,
    tx: &Sender<(HostId, HostEvent)>,
    stop: &AtomicBool,
    pgid: &AtomicI32,
) {
    let id: HostId = host.clone().unwrap_or_else(|| "local".into());
    let send = |event| tx.send((id.clone(), event)).is_ok();
    let mut backoff = Duration::from_secs(1);
    while !stop.load(Ordering::SeqCst) {
        let reason = match observe(host.as_deref(), pgid, &send) {
            Ok(Run { reason, healthy }) => {
                if healthy {
                    backoff = Duration::from_secs(1);
                }
                reason
            }
            Err(err) => {
                send(HostEvent::Failed(err));
                return;
            }
        };
        if stop.load(Ordering::SeqCst)
            || !send(HostEvent::Down {
                reason,
                retry_in: backoff,
            })
        {
            return;
        }
        let mut waited = Duration::ZERO;
        while waited < backoff && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
            waited += Duration::from_millis(50);
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

struct Run {
    reason: String,
    healthy: bool,
}

/// Runs one observer until it ends or goes silent. Err means the stream itself is unreadable.
fn observe(host: Option<&str>, pgid: &AtomicI32, send: &dyn Fn(HostEvent) -> bool) -> Result<Run> {
    let mut child = match start(host) {
        Ok(child) => child,
        Err(err @ Error::RexNotFound(_)) => return Err(err),
        Err(err) => {
            return Ok(Run {
                reason: err.to_string(),
                healthy: false,
            });
        }
    };
    pgid.store(child.id() as i32, Ordering::SeqCst);
    let stdout = child.stdout.take().expect("piped stdout");
    let (lines_tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut version = "unknown".to_owned();
    let mut healthy = false;
    let reason = loop {
        match lines.recv_timeout(SILENCE) {
            Ok(Ok(line)) if line.trim().is_empty() => {}
            Ok(Ok(line)) => match event::parse(&line, &version) {
                Ok(event) => {
                    if let Event::Snapshot { rex_version, .. } = &event {
                        version = rex_version.clone().unwrap_or(version);
                        healthy = true;
                    }
                    if !send(HostEvent::Event(event)) {
                        break "stopped".to_owned();
                    }
                }
                Err(err) => {
                    stop_child(&mut child, pgid);
                    return Err(err);
                }
            },
            Ok(Err(err)) => break format!("read failed: {err}"),
            Err(RecvTimeoutError::Timeout) => {
                break format!("no line for {}s", SILENCE.as_secs());
            }
            Err(RecvTimeoutError::Disconnected) => break "observer exited".to_owned(),
        }
    };
    let status = stop_child(&mut child, pgid);
    Ok(Run {
        reason: status.map_or(reason.clone(), |s| format!("{reason} ({s})")),
        healthy,
    })
}

fn start(host: Option<&str>) -> Result<Child> {
    let rex = rex::locate()?;
    let mut cmd = Command::new(&rex);
    if let Some(host) = host {
        cmd.args(["-S", host]);
    }
    let mut child = cmd
        .args(["do", "-", "--all"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|err| Error::RexNotFound(format!("could not run {}: {err}", rex.display())))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin.write_all(OBSERVER.as_bytes())?;
    Ok(child)
}

fn stop_child(child: &mut Child, pgid: &AtomicI32) -> Option<std::process::ExitStatus> {
    let status = child.try_wait().ok().flatten();
    kill_group(pgid.swap(0, Ordering::SeqCst));
    let _ = child.wait();
    status
}

/// Remote host labels from `rex hosts --json`.
pub fn hosts() -> Result<Vec<String>> {
    let rex = rex::locate()?;
    let out = Command::new(&rex)
        .args(["hosts", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| Error::RexNotFound(format!("could not run {}: {err}", rex.display())))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let why = text.lines().find(|line| !line.trim().is_empty());
        return Err(Error::Rex(format!(
            "rex hosts failed ({}): {}",
            out.status,
            why.unwrap_or("no output")
        )));
    }
    let value: Value = serde_json::from_slice(&out.stdout).map_err(|err| {
        Error::Rex(format!(
            "rex hosts printed something that is not JSON: {err}"
        ))
    })?;
    let mut labels = Vec::new();
    for entry in value["hosts"].as_array().into_iter().flatten() {
        let Some(label) = entry["host"]["label"].as_str() else {
            return Err(Error::Rex(format!(
                "rex hosts entry has no host.label: {entry}"
            )));
        };
        labels.push(label.to_owned());
    }
    Ok(labels)
}
