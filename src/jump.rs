use serde_json::{Value, json};

use crate::board::{Group, Row};
use crate::client::{self, Rows};
use crate::config;
use crate::error::{Error, Result};
use crate::output::{labels, path};
use crate::rex;

const SCRIPT: &str = concat!(
    include_str!("lua/app.lua"),
    "\n",
    include_str!("lua/jump.lua")
);

/// `rexboard jump`: TARGET is a key (`HOST/BLOCK_ID`), N, the nth NEEDS YOU row, or `-`.
pub fn jump(target: &str, json: bool) -> Result<()> {
    let row = if target == "-" {
        previous()?
    } else {
        let rows: Rows =
            serde_json::from_value(client::call(&json!({ "cmd": "rows" }))?).map_err(|err| {
                Error::serve(format!("serve sent rows in an unexpected shape: {err}"))
            })?;
        find(rows.rows, target)?
    };
    go(&row)?;
    done(&row, json);
    Ok(())
}

/// `rexboard next`: the NEEDS YOU row after this pane (`REX_BLOCK`), unseen rows first.
pub fn next(back: bool, json: bool) -> Result<()> {
    let after = std::env::var("REX_BLOCK").ok().filter(|id| !id.is_empty());
    let reply = client::call(&json!({ "cmd": "next", "after": after, "back": back }))?;
    let row: Row = serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))?;
    go(&row)?;
    done(&row, json);
    Ok(())
}

/// The pane the jump before the last went to. Going there makes it the last, so the next `-`
/// comes back, as `cd -` does.
pub fn previous() -> Result<Row> {
    let reply = client::call(&json!({ "cmd": "back" }))?;
    serde_json::from_value(reply["row"].clone())
        .map_err(|err| Error::serve(format!("serve sent a row in an unexpected shape: {err}")))
}

fn find(rows: Vec<Row>, target: &str) -> Result<Row> {
    if let Ok(n) = target.parse::<usize>() {
        let needs: Vec<Row> = rows
            .into_iter()
            .filter(|row| row.group == Group::NeedsYou)
            .collect();
        if needs.is_empty() {
            return Err(Error::NobodyNeedsYou);
        }
        let count = needs.len();
        return n
            .checked_sub(1)
            .and_then(|i| needs.into_iter().nth(i))
            .ok_or_else(|| Error::NotFound(format!("NEEDS YOU has {count} rows, not a row {n}")));
    }
    if !target.contains('/') {
        return Err(Error::usage_with(
            format!("TARGET is HOST/BLOCK_ID or a NEEDS YOU row number: {target}"),
            &["rexboard ls --json"],
        ));
    }
    rows.into_iter()
        .find(|row| row.key == target)
        .ok_or_else(|| Error::NotFound(format!("no pane {target}")))
}

/// Lands the Rex app on the row's pane and marks it seen.
pub fn go(row: &Row) -> Result<()> {
    let config = config::load()?;
    let app_client = app_client(config.client);
    let p = &row.pane;
    let reply = rex::run_script(
        None,
        SCRIPT,
        &json!({
            "host": p.host,
            "session_id": p.session_id,
            "window_id": p.window_id,
            "block_id": p.block_id,
            "app_client": app_client,
        }),
    )?;
    check(&reply, "jump", &format!("rexboard jump {}", row.key))?;
    client::call(&json!({ "cmd": "seen", "key": row.key, "jump": true }))?;
    Ok(())
}

/// The Rex app client to ask: `REX_CLIENT`, else `client` from config.toml.
fn app_client(configured: Option<String>) -> Option<String> {
    std::env::var("REX_CLIENT")
        .ok()
        .filter(|label| !label.is_empty())
        .or(configured)
}

/// Turns a Lua reply into an error the reader can act on. `what` names the action in Rex's own
/// failures, and `retry` is the command to run again once the app answers.
fn check(reply: &Value, what: &str, retry: &str) -> Result<()> {
    if reply["ok"] == true {
        return Ok(());
    }
    let labels: Vec<String> = reply["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|label| label.as_str().map(str::to_owned))
        .collect();
    let text = |field: &str| {
        reply[field]
            .as_str()
            .unwrap_or("no reason given")
            .to_owned()
    };
    let pick_one = || {
        labels
            .iter()
            .map(|label| format!("client = \"{label}\" in config.toml, or REX_CLIENT={label}"))
            .collect()
    };
    let (step, error) = (text("step"), text("error"));
    Err(match reply["code"].as_str() {
        Some("ambiguous_client") => Error::AmbiguousClient {
            message: format!(
                "more than one Rex app is on this host's unix socket: {}",
                labels.join(", ")
            ),
            steps: pick_one(),
        },
        Some("no_unix_app") => Error::AmbiguousClient {
            message: format!(
                "no Rex app is on this host's unix socket; app clients: {}",
                labels.join(", ")
            ),
            steps: pick_one(),
        },
        Some("no_app") => Error::AmbiguousClient {
            message: "no Rex app is connected to this host's Rex server".to_owned(),
            steps: vec!["open the Rex app".to_owned(), "rex client ls".to_owned()],
        },
        Some("no_client") => Error::Config(format!(
            "{}; the app clients are: {}",
            text("error"),
            labels.join(", ")
        )),
        _ if step == "session.select"
            && (error.contains("code=timeout") || error.contains("status=504")) =>
        {
            Error::RexExplained {
                message: format!(
                    "the Rex app on {} did not answer within 5s; it does not answer while the Mac's screen is locked",
                    app_name(&error)
                ),
                steps: vec![
                    format!("unlock the Mac, then {retry}"),
                    "rex client ls".to_owned(),
                ],
                detail: error,
            }
        }
        _ => Error::Rex(format!("{what} failed at {step}: {error}")),
    })
}

/// The app's name from Rex's timeout text, as in `"studio" did not answer`.
fn app_name(error: &str) -> String {
    error
        .split_once(" did not answer")
        .and_then(|(head, _)| head.rsplit('"').nth(1))
        .map_or_else(|| "this Mac".to_owned(), crate::output::clean)
}

/// How a jump names its row: `session › tab › pane on host`.
fn name(row: &Row) -> String {
    format!(
        "{} on {}",
        path(&labels(row, false), usize::MAX),
        row.pane.host
    )
}

pub fn done(row: &Row, json: bool) {
    if json {
        println!("{}", json!({ "ok": true, "key": row.key }));
    } else {
        println!("jumped to {}", name(row));
    }
}
