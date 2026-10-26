mod board;
mod cli;
mod client;
mod collector;
mod config;
mod error;
mod event;
mod extension;
mod jump;
mod output;
mod pane;
mod rex;
mod send;
mod serve;
mod show;
mod state;
mod tui;
mod upgrade;

pub use cli::Cli;
pub use error::{Error, report};

use std::io::{IsTerminal, Write, stderr, stdin, stdout};

use clap::CommandFactory;

use serde_json::json;

use std::collections::{BTreeMap, BTreeSet};
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
        if !cli.json && stdout().is_terminal() && stdin().is_terminal() {
            return tui::run();
        }
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
        Command::Ls(args) if args.direct => ls(args, cli.json),
        Command::Ls(args) => client::ls(args, cli.json),
        Command::Watch(args) if args.raw || args.direct => watch(args, cli.json),
        Command::Watch(args) if cli.json => client::watch(args),
        Command::Watch(args) => watch(args, cli.json),
        Command::Serve(args) if args.stop => client::stop_serve(cli.json),
        Command::Serve(args) if args.foreground => serve::run(),
        Command::Serve(_) => client::start_serve(cli.json),
        Command::Seen(args) => client::seen(args, cli.json),
        Command::Show { key, tail } => show::show(key, *tail, cli.json),
        Command::Views => show::views(cli.json),
        Command::Hosts => show::hosts(cli.json),
        Command::Jump { target } => jump::jump(target, cli.json),
        Command::Next { back } => jump::next(*back, cli.json),
        Command::Upgrade { check } => upgrade::run(*check, cli.json),
        Command::Ext(command) => extension::cli::run(command, cli.json),
        Command::Board | Command::Pick => {
            if !stdout().is_terminal() || !stdin().is_terminal() {
                return Err(Error::usage_with(
                    "the board needs a terminal",
                    &["rexboard ls", "rexboard jump N"],
                ));
            }
            if matches!(command, Command::Pick) {
                tui::pick(cli.json)
            } else {
                tui::run()
            }
        }
    }
}

fn ls(args: &LsArgs, json: bool) -> Result<()> {
    let exclude = config::load()?.hosts.exclude;
    let value = rex::run_script(
        None,
        rex::SNAPSHOT,
        &json!({ "host": args.host, "exclude": exclude }),
    )?;
    let rex_version = value["rex_version"].as_str().map(str::to_owned);
    let version = rex_version
        .clone()
        .unwrap_or_else(|| "version unknown".into());
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
            rex_version: rex_version.clone(),
        };
        board.apply(
            &host.host,
            &HostEvent::Event(snapshot, serde_json::Value::Null),
            now,
        )?;
    }
    for warning in board.take_warnings() {
        eprintln!("{warning}");
    }
    let rows: Vec<_> = board
        .rows(now)
        .into_iter()
        .filter(|row| args.all || row.pane.is_agent())
        .collect();
    if json {
        println!(
            "{}",
            serde_json::to_string(&rows).map_err(std::io::Error::other)?
        );
    } else {
        print!("{}", output::table(&rows, now, output::terminal_width()));
    }
    Ok(())
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// SIGINT, SIGTERM and SIGHUP set a flag the loops check, so they return and Drop cleans up.
pub(crate) fn stop_on_signals() {
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as *const () as libc::sighandler_t);
    }
}

pub(crate) fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Rows print at most this often under a burst of events.
const DEBOUNCE: Duration = Duration::from_millis(100);

fn watch(args: &WatchArgs, json: bool) -> Result<()> {
    if !args.raw && !json {
        return Err(Error::usage_with(
            "watch needs --json or --raw for now",
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
    stop_on_signals();
    let deadline = args
        .duration
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let (tx, rx) = mpsc::channel();
    let collectors: BTreeMap<String, Collector> = hosts
        .iter()
        .map(|host| {
            let id = host.as_deref().unwrap_or("local").to_owned();
            (id, Collector::spawn(host.clone(), tx.clone()))
        })
        .collect();
    drop(tx);
    let mut failed = BTreeSet::new();
    let mut out = std::io::stdout().lock();
    let mut board = Board::default();
    let mut due: Option<Instant> = None;
    while !interrupted() && deadline.is_none_or(|d| Instant::now() < d) {
        if due.is_some_and(|due| Instant::now() >= due) {
            due = None;
            if print_rows(&mut out, &board).is_err() {
                return Ok(());
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
        if failed.contains(&host) {
            continue;
        }
        if let HostEvent::Failed(err) = event {
            stop_host(&mut failed, collectors.len(), &host, err)?;
            if !args.raw && !board.mark_stale(&host).is_empty() {
                due.get_or_insert_with(|| Instant::now() + DEBOUNCE);
            }
            continue;
        }
        if !args.raw {
            let changed = match board.apply(&host, &event, chrono::Utc::now()) {
                Ok(changed) => changed,
                Err(err) => {
                    collectors[&host].restart(&host, &err, &event);
                    Vec::new()
                }
            };
            for warning in board.take_warnings() {
                eprintln!("{warning}");
            }
            if !changed.is_empty() {
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
            HostEvent::Failed(_) => continue,
        };
        line["host"] = json!(host);
        line["v"] = json!(event::VERSION);
        if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
            return Ok(());
        }
    }
    if due.is_some() {
        let _ = print_rows(&mut out, &board);
    }
    Ok(())
}

/// One host's bad line stops that host only; watch fails once every host has.
fn stop_host(failed: &mut BTreeSet<String>, hosts: usize, host: &str, err: Error) -> Result<()> {
    failed.insert(host.to_owned());
    if failed.len() == hosts {
        return Err(err);
    }
    eprintln!("warning: {host} stopped and will not restart: {err}");
    Ok(())
}

fn print_rows(out: &mut impl Write, board: &Board) -> std::io::Result<()> {
    let rows: Vec<_> = board
        .rows(chrono::Utc::now())
        .into_iter()
        .filter(|row| row.pane.is_agent())
        .collect();
    writeln!(out, "{}", serde_json::to_string(&rows)?)?;
    out.flush()
}
