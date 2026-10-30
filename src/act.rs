//! `rexboard answer` and `close`: the board's answer mode and `dd` for agents and scripts, with
//! the same checks before anything reaches a pane, and the screen the caller read pinned by its
//! `--dialog`.

use std::io::{BufRead, IsTerminal, Write, stderr, stdin};

use serde_json::json;

use crate::board::Row;
use crate::cli::AnswerArgs;
use crate::client;
use crate::error::{Error, Result};
use crate::output::clean;
use crate::send::{self, Action};
use crate::show::{find, place};
use crate::tui::answer::{dialog_id, expect, no_option, options_json, text_refusal};
use crate::tui::screen::Screen;
use crate::tui::{RUNS_THE_BOARD, ending, is_own, read_screen, runs_rexboard, tmux_note};

pub fn answer(args: &AnswerArgs, json: bool) -> Result<()> {
    let (row, _) = find(&args.key)?;
    let key = row.key.as_str();
    // Inside tmux the caller's own pane is unknown (see may_be_own); answering it is harmless,
    // since a pane running this command is not waiting on a dialog.
    if is_callers(&row) {
        return Err(refused("that is your own pane; nothing sent", &[]));
    }
    if row.pane.updated_at.is_none() {
        return Err(refused(
            "this pane reports no status, so it cannot be answered; nothing sent",
            &[format!("rexboard jump {key}")],
        ));
    }
    if row.pane.tmux && row.pane.process.as_deref() != Some("tmux") {
        return Err(refused(send::WRAPPED, &[format!("rexboard jump {key}")]));
    }
    if let Some(want) = &args.expect {
        expected(&row, want)?;
    }
    let screen = read_screen(&row)?;
    let reread = [format!("rexboard show {key} --json")];
    let nothing_there = || {
        refused(
            "no dialog or input line on the screen now; nothing sent",
            &reread,
        )
    };
    let id = dialog_id(&screen, row.pane.updated_at.as_deref()).ok_or_else(nothing_there)?;
    if id != args.dialog {
        let what = if screen.options.is_empty() {
            "input line"
        } else {
            "dialog"
        };
        return Err(refused(
            format!("the {what} changed since you read it; nothing sent"),
            &reread,
        ));
    }
    let (action, label) = reply(args, &screen, key)?;
    let expect = expect(&row, &screen).ok_or_else(nothing_there)?;
    send::send(&crate::rex::locate()?, &row.pane, &expect, &action)
        .map_err(|why| refused(why, &reread))?;
    // A reply marks the row seen, as on the board.
    let _ = client::call(&json!({ "cmd": "seen", "key": key }));
    let sent = match &action {
        Action::Pick(digit) => json!({ "pick": digit.to_digit(10), "label": label }),
        Action::Text(text) => json!({ "text": text }),
    };
    if json {
        println!("{}", json!({ "ok": true, "key": key, "sent": sent }));
    } else {
        let what = match &action {
            Action::Pick(digit) => format!("{digit} ({})", label.unwrap_or_default()),
            Action::Text(text) => format!("\"{text}\""),
        };
        println!("sent {what} to {} on {}", place(&row), row.pane.host);
    }
    Ok(())
}

pub fn close(key: &str, yes: bool, json: bool) -> Result<()> {
    let (row, _) = find(key)?;
    if runs_rexboard(&row) || is_callers(&row) {
        return Err(refused(RUNS_THE_BOARD, &[]));
    }
    if may_be_own(&row) {
        return Err(refused(
            "you are inside tmux and this pane runs tmux, so it may be the one you are in; it stays open",
            &[],
        ));
    }
    let question = format!(
        "close {} on {}{}?{}",
        place(&row),
        row.pane.host,
        ending(&row),
        tmux_note(&row)
    );
    if !yes {
        if !stdin().is_terminal() || !stderr().is_terminal() {
            let with_yes = format!("rexboard close {key} --yes");
            return Err(Error::usage_with(
                format!("{question} Without a terminal to ask on, --yes closes it"),
                &[with_yes.as_str()],
            ));
        }
        eprint!("{question} [y/N] ");
        stderr().flush()?;
        let mut reply = String::new();
        stdin().lock().read_line(&mut reply)?;
        if reply.trim() != "y" {
            eprintln!("nothing closed");
            return Err(Error::Cancelled);
        }
    }
    send::close(&crate::rex::locate()?, &row.pane).map_err(Error::Rex)?;
    if json {
        println!("{}", json!({ "ok": true, "key": row.key, "closed": true }));
    } else {
        println!("closed {} on {}", place(&row), row.pane.host);
    }
    Ok(())
}

/// What `--pick` or `--text` sends to `screen`, and the option's label for a pick; or why
/// nothing goes.
fn reply(args: &AnswerArgs, screen: &Screen, key: &str) -> Result<(Action, Option<String>)> {
    let id = &args.dialog;
    let reread = [format!("rexboard show {key} --json")];
    let with_options = |why: &str| {
        let list: Vec<String> = screen
            .options
            .iter()
            .map(|c| format!("{} {}", c.digit, c.label))
            .collect();
        Error::Refused {
            message: format!("{why}; options: {}", list.join(" · ")),
            steps: vec![format!("rexboard answer {key} --pick 1 --dialog {id}")],
            options: Some(options_json(screen)),
        }
    };
    Ok(match (args.pick, &args.text) {
        (Some(n), _) => {
            let digit = char::from(b'0' + n);
            if screen.options.is_empty() {
                return Err(refused(
                    "no dialog on the screen; --text replies to an input line; nothing sent",
                    &reread,
                ));
            }
            let Some(choice) = screen.options.iter().find(|c| c.digit == digit) else {
                return Err(with_options(&format!("{}; nothing sent", no_option(digit))));
            };
            if choice.lasting() && !args.confirm {
                return Err(refused(
                    format!(
                        "option {digit} ({}) grants something for later; add --confirm to send it; nothing sent",
                        choice.label
                    ),
                    &[format!(
                        "rexboard answer {key} --pick {digit} --confirm --dialog {id}"
                    )],
                ));
            }
            (Action::Pick(digit), Some(choice.label.clone()))
        }
        (None, Some(text)) => {
            let text = clean(text).trim().to_owned();
            if text.is_empty() {
                return Err(Error::usage_with(
                    "--text is empty",
                    &["rexboard answer KEY --text 'run the tests' --dialog ID"],
                ));
            }
            if !screen.options.is_empty() {
                return Err(with_options(
                    "a dialog is up and takes no text; pick a number; nothing sent",
                ));
            }
            if let Some(why) = text_refusal(screen) {
                return Err(refused(why, &reread));
            }
            (Action::Text(text), None)
        }
        (None, None) => unreachable!("clap requires --pick or --text"),
    })
}

fn inside_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Whether `row` is the pane this command runs in, which REX_BLOCK names outside tmux.
fn is_callers(row: &Row) -> bool {
    let block = std::env::var("REX_BLOCK").ok().filter(|id| !id.is_empty());
    !inside_tmux() && is_own(row, block.as_deref())
}

/// Whether `row` may be the pane the caller runs in. Inside tmux, REX_BLOCK names the pane that
/// started the tmux server, not the caller's, so any local pane running tmux may be it.
fn may_be_own(row: &Row) -> bool {
    inside_tmux() && row.pane.host == "local" && row.pane.in_tmux()
}

/// `--expect STATE[:KIND]`: the row must still be in it.
fn expected(row: &Row, want: &str) -> Result<()> {
    let (state, kind) = match want.split_once(':') {
        Some((state, kind)) => (state, Some(kind)),
        None => (want, None),
    };
    let is = &row.pane;
    if is.state == state && kind.is_none_or(|kind| is.kind.as_deref() == Some(kind)) {
        return Ok(());
    }
    let now = match &is.kind {
        Some(kind) => format!("{}:{kind}", is.state),
        None => is.state.clone(),
    };
    Err(refused(
        format!("{} is {now}, not {want}; nothing sent", row.key),
        &[format!("rexboard show {}", row.key)],
    ))
}

fn refused(message: impl Into<String>, steps: &[String]) -> Error {
    Error::Refused {
        message: message.into(),
        steps: steps.to_vec(),
        options: None,
    }
}
