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
        Command::Watch(args) => watch(args),
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
    let panes = pane::flatten(up, args.all);
    if json {
        println!(
            "{}",
            serde_json::to_string(&panes).map_err(std::io::Error::other)?
        );
    } else {
        print!("{}", output::table(&panes, chrono::Utc::now()));
    }
    Ok(())
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn watch(args: &WatchArgs) -> Result<()> {
    if !args.raw {
        return Err(Error::usage_with(
            "watch needs --raw for now",
            &["rexboard watch --raw"],
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
    while !INTERRUPTED.load(Ordering::SeqCst) && deadline.is_none_or(|d| Instant::now() < d) {
        let (host, event) = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(next) => next,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
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
