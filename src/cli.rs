use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rexboard",
    version,
    about = "Unofficial attention board for agent panes across Rex hosts.",
    after_help = "Errors: {\"ok\":false,\"error\":{code,message,next_steps}} on stderr with --json. Exit 1: Rex or serve failed, or the key was not found. Exit 2: bad usage, or every host is unreachable."
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
    /// Stream the agent rows (`--json`) or raw events (`--raw`) from this host and every `rex hosts` entry.
    Watch(WatchArgs),
    /// Run the board daemon behind ls, watch --json and seen. They start it when it is not running.
    Serve(ServeArgs),
    /// Mark a pane seen, by the `key` from `ls --json` (HOST/BLOCK_ID).
    Seen {
        #[arg(value_name = "KEY")]
        key: String,
    },
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Run in this terminal instead of in the background.
    #[arg(long, conflicts_with = "stop")]
    pub foreground: bool,
    /// Stop the running serve and its observers.
    #[arg(long)]
    pub stop: bool,
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
    /// Run this command's own observers instead of going through serve. `--raw` always does.
    #[arg(long)]
    pub direct: bool,
}

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Include plain shells, not just panes with a status record or a known harness.
    #[arg(long)]
    pub all: bool,
    /// Only this host: `local`, or a label from `rex hosts`.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Take one snapshot here instead of asking serve.
    #[arg(long)]
    pub direct: bool,
}
