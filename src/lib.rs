mod board;
mod cli;
mod collector;
mod error;
mod event;
mod output;
mod pane;
mod rex;

pub use cli::Cli;
pub use error::{Error, report};

use std::io::{Write, stderr};

use clap::CommandFactory;

use serde_json::json;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::board::Board;
use crate::cli::{Command, LsArgs, WatchArgs};
use crate::collector::{Collector, HostEvent};
use crate::error::Result;
use crate::event::Event;
use crate::pane::Snapshot;

pub fn wants_json_raw(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--json")
}

pub fn execute(cli: &Cli) -> Result<()> {
    let Some(command) = &cli.command else {
        if cli.json {
            return Err(Error::usage_with(
                "a command is required",
                &["rexboard --help"],
            ));
        }
        Cli::command().write_help(&mut stderr())?;
        writeln!(stderr())?;
        return Err(Error::Help);
    };
    match command {
        Command::Ls(args) => ls(args, cli.json),
        Command::Watch(args) => watch(args, cli.json),
    }
}

fn ls(args: &LsArgs, json: bool) -> Result<()> {
    let value = rex::run_script(None, rex::SNAPSHOT, &json!({ "host": args.host }))?;
    let version = value["rex_version"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let snapshot: Snapshot = serde_json::from_value(value).map_err(|err| {
        Error::Rex(format!(
            "unexpected snapshot shape from Rex {version}: {err}"
        ))
    })?;
    if snapshot.hosts.is_empty() {
        let host = args.host.as_deref().unwrap_or("local");
        return Err(Error::usage_with(
            format!("no host named {host}"),
            &["rex hosts", "rexboard ls"],
        ));
    }
    let (down, up): (Vec<_>, Vec<_>) = snapshot.hosts.into_iter().partition(|h| h.error.is_some());
    if up.is_empty() {
        let names: Vec<_> = down.iter().map(|h| h.host.as_str()).collect();
        return Err(Error::HostsUnreachable(format!(
            "every host is unreachable: {}",
            names.join(", ")
        )));
    }
    for host in &down {
        eprintln!(
            "warning: {} is unreachable: {}",
            host.host,
            host.error.as_deref().unwrap_or_default()
        );
    }
    let mut board = Board::default();
    let now = chrono::Utc::now();
    for host in up {
        let snapshot = Event::Snapshot {
            sessions: host.sessions,
            rex_version: Some(version.clone()),
        };
        board.apply(
            &host.host,
            &HostEvent::Event(snapshot, serde_json::Value::Null),
            now,
        )?;
    }
    let rows: Vec<_> = board
        .rows()
        .into_iter()
        .filter(|row| args.all || row.pane.is_agent())
        .collect();
    if json {
        println!(
            "{}",
            serde_json::to_string(&rows).map_err(std::io::Error::other)?
        );
    } else {
        let panes: Vec<_> = rows.into_iter().map(|row| row.pane).collect();
        print!("{}", output::table(&panes, now));
    }
    Ok(())
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Rows print at most this often under a burst of events.
const DEBOUNCE: Duration = Duration::from_millis(100);

fn watch(args: &WatchArgs, json: bool) -> Result<()> {
    if !args.raw && !json {
        return Err(Error::usage_with(
            "watch needs --raw or --json for now",
            &["rexboard watch --json", "rexboard watch --raw"],
        ));
    }
    let mut hosts: Vec<Option<String>> = vec![None];
    hosts.extend(collector::hosts()?.into_iter().map(Some));
    if let Some(want) = &args.host {
        hosts.retain(|host| host.as_deref().unwrap_or("local") == want);
        if hosts.is_empty() {
            return Err(Error::usage_with(
                format!("no host named {want}"),
                &["rex hosts", "rexboard watch --raw"],
            ));
        }
    }
    // Observers run in their own process group, so stop on a signal and let Drop kill them.
    unsafe {
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
        libc::signal(
            libc::SIGTERM,
            interrupted as *const () as libc::sighandler_t,
        );
    }
    let deadline = args
        .duration
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let (tx, rx) = mpsc::channel();
    let collectors: Vec<Collector> = hosts
        .iter()
        .map(|host| Collector::spawn(host.clone(), tx.clone()))
        .collect();
    drop(tx);
    let mut failed = 0;
    let mut out = std::io::stdout().lock();
    let mut board = Board::default();
    let mut due: Option<Instant> = None;
    while !INTERRUPTED.load(Ordering::SeqCst) && deadline.is_none_or(|d| Instant::now() < d) {
        if due.is_some_and(|due| Instant::now() >= due) {
            due = None;
            let rows: Vec<_> = board
                .rows()
                .into_iter()
                .filter(|r| r.pane.is_agent())
                .collect();
            let rows = serde_json::to_string(&rows).map_err(std::io::Error::other)?;
            if writeln!(out, "{rows}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
        let wait = due.map_or(Duration::from_millis(100), |due| {
            due.saturating_duration_since(Instant::now())
        });
        let (host, event) = match rx.recv_timeout(wait) {
            Ok(next) => next,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if !args.raw {
            if let HostEvent::Failed(err) = event {
                failed += 1;
                if failed == collectors.len() {
                    return Err(err);
                }
                eprintln!("warning: {host} stopped and will not restart: {err}");
            } else if !board.apply(&host, &event, chrono::Utc::now())?.is_empty() {
                due.get_or_insert_with(|| Instant::now() + DEBOUNCE);
            }
            continue;
        }
        let mut line = match event {
            // An unknown type passes through as Rex sent it.
            HostEvent::Event(Event::Unknown, raw) => raw,
            HostEvent::Event(event, _) => {
                serde_json::to_value(event).map_err(std::io::Error::other)?
            }
            HostEvent::Down { reason, retry_in } => {
                json!({ "type": "down", "reason": reason, "retry_in_s": retry_in.as_secs() })
            }
            HostEvent::Failed(err) => {
                failed += 1;
                if failed == collectors.len() {
                    return Err(err);
                }
                eprintln!("warning: {host} stopped and will not restart: {err}");
                continue;
            }
        };
        line["host"] = json!(host);
        line["v"] = json!(event::VERSION);
        if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
    Ok(())
}
