use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rexboard",
    version,
    about = "Unofficial attention board for agent panes across Rex hosts.",
    after_help = "Errors: {\"ok\":false,\"error\":{code,message,next_steps}} on stderr with --json. Exit 1: Rex failed. Exit 2: bad usage, or every host is unreachable."
)]
pub struct Cli {
    /// Print JSON on stdout, and errors as JSON on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List agent panes on this host and every `rex hosts` entry, most urgent first.
    Ls(LsArgs),
    /// Stream events from this host and every `rex hosts` entry as they happen.
    Watch(WatchArgs),
}

#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Print one JSON line per event, tagged with its `host`.
    #[arg(long)]
    pub raw: bool,
    /// Only this host: `local`, or a label from `rex hosts`.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Stop after this many seconds. Without it, watch runs until interrupted.
    #[arg(long = "for", value_name = "SECONDS")]
    pub duration: Option<u64>,
}

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Include plain shells, not just panes with a status record or a known harness.
    #[arg(long)]
    pub all: bool,
    /// Only this host: `local`, or a label from `rex hosts`.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
}
