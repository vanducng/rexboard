mod cli;
mod error;

pub use cli::Cli;
pub use error::{Error, report};

use std::io::{Write, stderr};

use clap::CommandFactory;

use crate::error::Result;

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
    match *command {}
}
