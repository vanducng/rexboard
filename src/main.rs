use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // `hook` and `usage` run inside an agent's hook or status line, where exit 2 blocks the
    // agent: they say what went wrong and exit 0, a status line command's own code aside.
    let fails_open = matches!(
        args.iter()
            .skip(1)
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str),
        Some("hook" | "usage")
    );
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
        Err(err) if fails_open => {
            eprintln!("{}", err.render());
            rexboard::hook_note(&summary(&err.render().to_string()));
            return run_after_dashes(&args);
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
        Err(err) if fails_open => {
            rexboard::report(false, &err);
            rexboard::hook_note(&err.to_string());
            ExitCode::SUCCESS
        }
        Err(err) => {
            if !err.quiet() {
                rexboard::report(cli.json, &err);
            }
            err.exit_code()
        }
    }
}

/// A status line's own command after `--`, run as `usage` would have run it, when the words
/// before it are ones this build cannot read: its output and exit code are the status line's.
fn run_after_dashes(args: &[String]) -> ExitCode {
    let Some(at) = args.iter().position(|arg| arg == "--") else {
        return ExitCode::SUCCESS;
    };
    let Some((cmd, rest)) = args[at + 1..].split_first() else {
        return ExitCode::SUCCESS;
    };
    match std::process::Command::new(cmd).args(rest).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(_) => ExitCode::SUCCESS,
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
