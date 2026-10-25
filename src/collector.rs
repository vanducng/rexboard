use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::event::{self, Event};
use crate::rex;

pub const OBSERVER: &str = concat!(
    include_str!("lua/lib.lua"),
    "\n",
    include_str!("lua/observer.lua")
);

const TICK: Duration = Duration::from_secs(30);
/// Three missed ticks means the host is gone.
const SILENCE: Duration = Duration::from_secs(90);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Unreadable lines logged per host; later ones are skipped silently.
const MAX_LOGGED_SKIPS: u32 = 5;

pub type HostId = String;

#[derive(Debug)]
pub enum HostEvent {
    /// A parsed line and the object it came from.
    Event(Event, Value),
    /// The observer stopped; a new one starts after `retry_in`.
    Down { reason: String, retry_in: Duration },
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

    /// The running observer's pid, which is also its process group; 0 between runs.
    pub fn pid(&self) -> i32 {
        self.pgid.load(Ordering::SeqCst)
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
    let mut skipped = 0;
    while !stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let reason = match observe(host.as_deref(), stop, pgid, &mut skipped, &send) {
            Ok(reason) => reason,
            Err(err) => {
                send(HostEvent::Failed(err));
                return;
            }
        };
        // A run that fails on every event dies fast, so only a run that outlived a tick resets.
        if started.elapsed() >= TICK {
            backoff = Duration::from_secs(1);
        }
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

/// Runs one observer until it ends or goes silent, and returns why. Err means rex is missing or speaks another contract.
fn observe(
    host: Option<&str>,
    stop: &AtomicBool,
    pgid: &AtomicI32,
    skipped: &mut u32,
    send: &dyn Fn(HostEvent) -> bool,
) -> Result<String> {
    let mut child = match start(host) {
        Ok(child) => child,
        Err(err @ Error::RexNotFound(_)) => return Err(err),
        Err(err) => return Ok(err.to_string()),
    };
    pgid.store(child.id() as i32, Ordering::SeqCst);
    if stop.load(Ordering::SeqCst) {
        stop_child(&mut child, pgid);
        return Ok("stopped".into());
    }
    let stderr = last_line(child.stderr.take().expect("piped stderr"));
    let stdout = child.stdout.take().expect("piped stdout");
    let (lines_tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut version = "version unknown".to_owned();
    let reason = loop {
        match lines.recv_timeout(SILENCE) {
            Ok(Ok(line)) if line.trim().is_empty() => {}
            Ok(Ok(line)) => match event::parse(&line, &version) {
                Ok((event, raw)) => {
                    if let Event::Snapshot {
                        rex_version: Some(v),
                        ..
                    } = &event
                    {
                        version.clone_from(v);
                    }
                    if !send(HostEvent::Event(event, raw)) {
                        break "stopped".to_owned();
                    }
                }
                Err(err @ Error::ContractMismatch(_)) => {
                    stop_child(&mut child, pgid);
                    return Err(err);
                }
                Err(err) => skip(host.unwrap_or("local"), &line, &err, skipped),
            },
            Ok(Err(err)) => break format!("read failed: {err}"),
            Err(RecvTimeoutError::Timeout) => {
                break format!("no line for {}s", SILENCE.as_secs());
            }
            Err(RecvTimeoutError::Disconnected) => break "observer exited".to_owned(),
        }
    };
    if reason == "observer exited" {
        exit_soon(&mut child);
    }
    let status = stop_child(&mut child, pgid);
    let mut reason = status.map_or(reason.clone(), |s| format!("{reason} ({s})"));
    if let Some(line) = stderr.join().ok().flatten() {
        reason = format!("{reason}: {line}");
    }
    Ok(reason)
}

/// One unreadable line must not stop the host. The first few go to stderr (serve.log) with their text.
fn skip(id: &str, line: &str, err: &Error, skipped: &mut u32) {
    *skipped += 1;
    if *skipped > MAX_LOGGED_SKIPS {
        return;
    }
    let text: String = line.chars().take(500).collect();
    eprintln!("warning: {id}: skipped an observer line: {err}: {text}");
    if *skipped == MAX_LOGGED_SKIPS {
        eprintln!("warning: {id}: later skipped lines are not logged");
    }
}

/// The last non-empty stderr line, skipping the banner `rex do` prints when it starts listening.
fn last_line(pipe: impl Read + Send + 'static) -> thread::JoinHandle<Option<String>> {
    thread::spawn(move || {
        BufReader::new(pipe)
            .lines()
            .map_while(std::result::Result::ok)
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty() && !line.starts_with("listening:"))
            .last()
    })
}

fn start(host: Option<&str>) -> Result<Child> {
    let rex = rex::locate()?;
    let mut cmd = rex::command(&rex);
    if let Some(host) = host {
        cmd.args(["-S", host]);
    }
    cmd.args(["do", "-", "--all", "--fail-fast"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = rex::spawn(&mut cmd)
        .map_err(|err| Error::RexNotFound(format!("could not run {}: {err}", rex.display())))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    if let Err(err) = stdin.write_all(OBSERVER.as_bytes()) {
        kill_group(child.id() as i32);
        let _ = child.wait();
        return Err(err.into());
    }
    Ok(child)
}

/// stdout can close a moment before the process is reaped; wait up to 1s so the reason has its status.
fn exit_soon(child: &mut Child) {
    for _ in 0..100 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn stop_child(child: &mut Child, pgid: &AtomicI32) -> Option<std::process::ExitStatus> {
    let status = child.try_wait().ok().flatten();
    kill_group(pgid.swap(0, Ordering::SeqCst));
    let _ = child.wait();
    status
}

/// Remote host labels from `rex hosts --json`, less `hosts.exclude` in config.toml.
pub fn hosts() -> Result<Vec<String>> {
    let exclude = crate::config::load()?.hosts.exclude;
    let rex = rex::locate()?;
    let mut cmd = rex::command(&rex);
    cmd.args(["hosts", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = rex::spawn(&mut cmd)
        .and_then(Child::wait_with_output)
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
        if !exclude.iter().any(|skip| skip == label) {
            labels.push(label.to_owned());
        }
    }
    Ok(labels)
}
