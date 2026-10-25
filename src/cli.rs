use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rexboard",
    version,
    about = "Unofficial attention board for agent panes across Rex hosts.",
    after_help = "Errors: {\"ok\":false,\"error\":{code,message,next_steps}} on stderr with --json. Usage errors exit 2."
)]
pub struct Cli {
    /// Print one JSON object on stdout, and errors as JSON on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {}
