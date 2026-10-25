use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cli = match rexboard::Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            err.exit()
        }
        Err(err) if rexboard::wants_json_raw(&args) => {
            let usage = rexboard::Error::usage_with(
                summary(&err.render().to_string()),
                &["rexboard --help"],
            );
            rexboard::report(true, &usage);
            return usage.exit_code();
        }
        Err(err) => err.exit(),
    };
    match rexboard::execute(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if !err.quiet() {
                rexboard::report(cli.json, &err);
            }
            err.exit_code()
        }
    }
}

/// Clap's message without the usage block, on one line, so an agent sees which argument is wrong.
fn summary(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with("Usage:") && !line.starts_with("For more information"))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches("error: ")
        .to_owned()
}
