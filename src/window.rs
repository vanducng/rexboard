use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

use crate::config;
use crate::error::{Error, Result};
use crate::jump;
use crate::rex;

const SCRIPT: &str = concat!(
    include_str!("lua/app.lua"),
    "\n",
    include_str!("lua/window.lua")
);

/// The board's own Rex session, and the tab its board runs in.
pub const SESSION: &str = "rexboard";
pub const TAB: &str = "board";

/// The app titles a window `<pane title> - <session>`, and the board titles its pane after
/// the tab, so a window manager can match this.
pub fn title() -> String {
    format!("{TAB} - {SESSION}")
}

/// `rexboard window`: show the board in its own Rex app window. The `rexboard` session is made
/// when it is missing. With `new`, or a session just made, a new app window opens for it;
/// otherwise the session is selected, which focuses a window that already shows it.
pub fn window(new: bool, print_title: bool, json: bool) -> Result<()> {
    if print_title {
        if json {
            println!("{}", json!({ "ok": true, "title": title() }));
        } else {
            println!("{}", title());
        }
        return Ok(());
    }
    let (board, made) = match find()? {
        Some(board) => (board, false),
        None => {
            create()?;
            let board = find()?.ok_or_else(|| {
                Error::Rex(format!(
                    "rex new did not leave a session labelled {SESSION}"
                ))
            })?;
            (board, true)
        }
    };
    let config = config::load()?;
    let reply = rex::run_script(
        None,
        SCRIPT,
        &json!({
            "mode": if new || made { "new" } else { "select" },
            "session_id": board.0,
            "window_id": board.1,
            "app_client": jump::app_client(config.client),
        }),
    )?;
    stray(&reply)?;
    jump::check(&reply, "window", "rexboard window")?;
    let opened = reply["opened"] == true;
    if json {
        println!(
            "{}",
            json!({ "ok": true, "opened": opened, "session_id": board.0, "title": title() })
        );
    } else if opened {
        println!("opened the board window: {}", title());
    } else {
        println!("showed the board: {}", title());
    }
    Ok(())
}

/// A new app window comes with a session of its own. window.lua destroys it; if that failed,
/// say which session is left so it can be closed by hand.
fn stray(reply: &Value) -> Result<()> {
    match reply["made"].as_str() {
        Some(made) if reply["destroyed"] != true => Err(Error::RexExplained {
            message: format!("the new Rex window left an extra session behind: {made}"),
            steps: vec![format!("rex kill {made}")],
            detail: reply["error"].as_str().unwrap_or_default().to_owned(),
        }),
        _ => Ok(()),
    }
}

/// The `rexboard` session on this host and its `board` tab (else its first tab).
fn find() -> Result<Option<(String, String)>> {
    let snapshot = rex::run_script(None, rex::SNAPSHOT, &json!({ "host": "local" }))?;
    let sessions = snapshot["hosts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|host| host["host"] == "local")
        .flat_map(|host| host["sessions"].as_array().into_iter().flatten());
    for session in sessions {
        if session["label"] != SESSION {
            continue;
        }
        let windows: Vec<&Value> = session["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let tab = windows
            .iter()
            .find(|w| w["label"] == TAB)
            .or_else(|| windows.first())
            .and_then(|w| w["window_id"].as_str());
        if let (Some(id), Some(tab)) = (session["session_id"].as_str(), tab) {
            return Ok(Some((id.to_owned(), tab.to_owned())));
        }
    }
    Ok(None)
}

/// Makes the `rexboard` session with the board running in its `board` tab.
fn create() -> Result<()> {
    let rex = rex::locate()?;
    let me = std::env::current_exe()?;
    let mut cmd = Command::new(&rex);
    cmd.args(["new", SESSION, "--window", TAB, "--"])
        .arg(&me)
        .arg("board")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let out = rex::spawn(&mut cmd)
        .and_then(Child::wait_with_output)
        .map_err(|err| Error::RexNotFound(format!("could not run {}: {err}", rex.display())))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let why = text.lines().find(|line| !line.trim().is_empty());
        return Err(Error::Rex(format!(
            "rex new {SESSION} failed ({}): {}",
            out.status,
            why.unwrap_or("no output")
        )));
    }
    Ok(())
}
