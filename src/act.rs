//! `rexboard answer` and `close`: the board's answer mode and `dd` for agents and scripts, with
//! the same checks before anything reaches a pane.

use std::io::{BufRead, IsTerminal, Write, stderr, stdin};

use serde_json::json;

use crate::board::Row;
use crate::cli::AnswerArgs;
use crate::client;
use crate::error::{Error, Result};
use crate::output::clean;
use crate::send::{self, Action};
use crate::show::{find, place};
use crate::tui::answer::{expect, no_option, text_refusal};
use crate::tui::screen::Screen;
use crate::tui::{RUNS_THE_BOARD, ending, is_own, read_screen, runs_rexboard, tmux_note};

pub fn answer(args: &AnswerArgs, json: bool) -> Result<()> {
    let (row, _) = find(&args.key)?;
    let refuse = |message: String| refused(&row, message);
    if is_own(&row, own().as_deref()) {
        return Err(refuse("that is your own pane; nothing sent".into()));
    }
    if row.pane.updated_at.is_none() {
        return Err(refuse(
            "this pane reports no status, so it cannot be answered; nothing sent".into(),
        ));
    }
    if let Some(want) = &args.expect {
        expected(&row, want)?;
    }
    let screen = read_screen(&row)?;
    let (action, label) = reply(args, &screen, &row)?;
    let expect = expect(&row, &screen)
        .ok_or_else(|| refuse("no dialog or input line on the screen; nothing sent".into()))?;
    send::send(&crate::rex::locate()?, &row.pane, &expect, &action).map_err(refuse)?;
    // A reply marks the row seen, as on the board.
    let _ = client::call(&json!({ "cmd": "seen", "key": row.key }));
    let sent = match &action {
        Action::Pick(digit) => json!({ "pick": digit.to_digit(10), "label": label }),
        Action::Text(text) => json!({ "text": text }),
    };
    if json {
        println!("{}", json!({ "ok": true, "key": row.key, "sent": sent }));
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
    if is_own(&row, own().as_deref()) || runs_rexboard(&row) {
        return Err(refused(&row, RUNS_THE_BOARD.into()));
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
        if !matches!(reply.trim(), "y" | "yes") {
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
fn reply(args: &AnswerArgs, screen: &Screen, row: &Row) -> Result<(Action, Option<String>)> {
    let refuse = |message: String| refused(row, message);
    Ok(match (args.pick, &args.text) {
        (Some(n), _) => {
            let digit = char::from(b'0' + n);
            let Some(choice) = screen.options.iter().find(|c| c.digit == digit) else {
                return Err(refuse(if screen.options.is_empty() {
                    "no dialog on the screen; --text replies to an input line; nothing sent".into()
                } else {
                    format!("{}; nothing sent", no_option(digit))
                }));
            };
            if choice.lasting() && !args.confirm {
                return Err(refuse(format!(
                    "option {digit} ({}) grants something for later; add --confirm to send it; nothing sent",
                    choice.label
                )));
            }
            (Action::Pick(digit), Some(choice.label.clone()))
        }
        (None, Some(text)) => {
            let text = clean(text).trim().to_owned();
            if text.is_empty() {
                return Err(Error::usage_with(
                    "--text is empty",
                    &["rexboard answer KEY --text 'run the tests'"],
                ));
            }
            if let Some(why) = text_refusal(screen) {
                return Err(refuse(why.to_owned()));
            }
            (Action::Text(text), None)
        }
        (None, None) => unreachable!("clap requires --pick or --text"),
    })
}

/// The caller's own pane, which it never answers or closes, as the board never does its own.
fn own() -> Option<String> {
    std::env::var("REX_BLOCK").ok().filter(|id| !id.is_empty())
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
        row,
        format!("{} is {now}, not {want}; nothing sent", row.key),
    ))
}

fn refused(row: &Row, message: String) -> Error {
    Error::Refused {
        message,
        steps: vec![format!("rexboard show {} --tail 40", row.key)],
    }
}
