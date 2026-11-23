//! `rexboard mark`: letters a-z for panes, kept by serve (see letters.rs), and `jump @a`.

use serde_json::{Value, json};

use crate::board::Row;
use crate::cli::MarkCommand;
use crate::client;
use crate::error::{Error, Result};
use crate::letters::letter;

fn named(text: &str) -> Result<char> {
    letter(text).ok_or_else(|| {
        Error::usage_with(
            format!("a mark is a letter from a to z, not {text:?}"),
            &["rexboard mark ls"],
        )
    })
}

pub fn run(command: &MarkCommand, json: bool) -> Result<()> {
    match command {
        MarkCommand::Set { letter, key } => {
            let letter = named(letter)?;
            let key = match key {
                Some(key) => key.clone(),
                None => std::env::var("REX_BLOCK")
                    .ok()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| {
                        Error::usage_with(
                            "no pane to mark: REX_BLOCK is not set outside a Rex pane",
                            &["rexboard mark set a --key HOST/BLOCK_ID"],
                        )
                    })?,
            };
            let reply = client::call(
                &json!({ "cmd": "letter", "op": "set", "letter": letter.to_string(), "key": key }),
            )?;
            let held = reply["key"].as_str().unwrap_or_default();
            if json {
                println!(
                    "{}",
                    json!({ "ok": true, "letter": letter.to_string(), "key": held })
                );
            } else {
                println!("mark {letter} is on {held}");
            }
            Ok(())
        }
        MarkCommand::Clear { letter } => {
            let letter = named(letter)?;
            client::call(&json!({ "cmd": "letter", "op": "clear", "letter": letter.to_string() }))?;
            if json {
                println!("{}", json!({ "ok": true, "cleared": letter.to_string() }));
            } else {
                println!("mark {letter} cleared");
            }
            Ok(())
        }
        MarkCommand::Ls => {
            let reply = client::call(&json!({ "cmd": "letter", "op": "ls" }))?;
            let letters = reply["letters"].as_array().cloned().unwrap_or_default();
            if json {
                println!("{}", Value::Array(letters));
                return Ok(());
            }
            if letters.is_empty() {
                println!("no letters · rexboard mark set a, in a pane");
            }
            for held in &letters {
                let text = |field: &str| held[field].as_str().unwrap_or_default();
                let gone = if held["on_board"] == true {
                    ""
                } else {
                    "  · not on the board"
                };
                println!(
                    "{}  {} › {} › {} › {}{gone}",
                    text("letter"),
                    text("host"),
                    text("session"),
                    text("tab"),
                    text("pane"),
                );
            }
            Ok(())
        }
    }
}

/// The row whose pane holds the letter that `jump @a` names.
pub fn pane_of(text: &str) -> Result<Row> {
    let letter = named(text)?;
    let reply =
        client::call(&json!({ "cmd": "letter", "op": "find", "letter": letter.to_string() }))?;
    serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))
}
