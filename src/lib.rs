mod cli;
mod error;
mod output;
mod pane;
mod rex;

pub use cli::Cli;
pub use error::{Error, report};

use std::io::{Write, stderr};

use clap::CommandFactory;

use serde_json::json;

use crate::cli::{Command, LsArgs};
use crate::error::Result;
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
