use std::collections::BTreeSet;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

/// How long a new session may take to show up: the app's own after a new window, or the board's
/// after `rex new`.
const APPEAR_WITHIN: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);

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
    let (board, made) = match board(&sessions()?) {
        Some(board) => (board, false),
        None => (create()?, true),
    };
    let app_client = jump::app_client(config::load()?.client);
    let opened = new || made;
    if opened {
        open(&board, app_client.as_deref())?;
    } else {
        call(
            &json!({ "mode": "select", "session_id": board.id, "window_id": board.tab, "app_client": app_client }),
        )?;
    }
    if json {
        println!(
            "{}",
            json!({ "ok": true, "opened": opened, "session_id": board.id, "title": title() })
        );
    } else if opened {
        println!("opened the board window: {}", title());
    } else {
        println!("showed the board: {}", title());
    }
    Ok(())
}

struct Board {
    id: String,
    tab: String,
}

/// A new app window comes with a session of its own. Open one, show the board in it, then
/// destroy that session: the one the app names, if it is new and not the board's, else the one
/// new session that looks like a fresh window's. Anything less certain destroys nothing and says
/// what it found.
fn open(board: &Board, app_client: Option<&str>) -> Result<()> {
    let before: BTreeSet<String> = sessions()?
        .iter()
        .filter_map(|s| s["session_id"].as_str().map(str::to_owned))
        .collect();
    let reply = call(&json!({ "mode": "open", "app_client": app_client }))?;
    let named = ["session_id", "session"]
        .iter()
        .find_map(|key| reply["reply"][*key].as_str())
        .filter(|id| *id != board.id && !before.contains(*id))
        .map(str::to_owned);
    let made = match named {
        Some(id) => id,
        None => new_window_session(&before, &board.id)?,
    };
    call(
        &json!({ "mode": "select", "session_id": board.id, "window_id": board.tab, "app_client": app_client }),
    )?;
    call(&json!({ "mode": "destroy", "session_id": made })).map_err(|err| Error::RexExplained {
        message: format!(
            "the board is shown, but the new window's own session {made} is left open"
        ),
        steps: vec![format!("rex kill {made}"), "rex ls".to_owned()],
        detail: err.to_string(),
    })?;
    Ok(())
}

/// The session a new app window came with: the one session that was not there before, once it
/// shows up, if it looks like a fresh window's (not the board's label, one tab, one pane running
/// a login shell, no status record).
fn new_window_session(before: &BTreeSet<String>, board: &str) -> Result<String> {
    let deadline = Instant::now() + APPEAR_WITHIN;
    loop {
        let now = sessions()?;
        let fresh: Vec<&Value> = now
            .iter()
            .filter(|s| {
                s["session_id"]
                    .as_str()
                    .is_some_and(|id| id != board && !before.contains(id))
            })
            .collect();
        let ids = || -> Vec<String> {
            fresh
                .iter()
                .filter_map(|s| s["session_id"].as_str().map(str::to_owned))
                .collect()
        };
        match fresh.as_slice() {
            [one] if looks_fresh(one) => return Ok(ids().remove(0)),
            [one] => {
                return Err(left(
                    format!(
                        "a session appeared that does not look like the new window's own ({}); nothing was destroyed",
                        describe(one)
                    ),
                    &ids(),
                ));
            }
            [] if Instant::now() < deadline => thread::sleep(POLL),
            [] => {
                return Err(Error::RexExplained {
                    message: format!(
                        "the new Rex window's own session did not show up within {}s; nothing was destroyed",
                        APPEAR_WITHIN.as_secs()
                    ),
                    steps: vec!["rex ls".to_owned()],
                    detail: String::new(),
                });
            }
            _ => {
                return Err(left(
                    format!(
                        "more than one session appeared with the new window: {}; nothing was destroyed",
                        ids().join(", ")
                    ),
                    &ids(),
                ));
            }
        }
    }
}

fn left(message: String, ids: &[String]) -> Error {
    let mut steps = vec!["rex ls".to_owned()];
    steps.extend(
        ids.iter()
            .map(|id| format!("rex kill {id}, if it is the new window's")),
    );
    Error::RexExplained {
        message,
        steps,
        detail: String::new(),
    }
}

fn panes(session: &Value) -> impl Iterator<Item = &Value> {
    session["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|w| w["blocks"].as_array().into_iter().flatten())
}

const SHELLS: [&str; 4] = ["zsh", "bash", "fish", "sh"];

fn is_shell(name: &str) -> bool {
    SHELLS.contains(&name.trim_start_matches('-'))
}

/// A shell pane's title is empty, the shell's name, or a directory, alone or after
/// `user@host: `.
fn shell_title(title: &str) -> bool {
    let dir = title.rsplit(": ").next().unwrap_or(title);
    title.is_empty() || is_shell(title) || dir.starts_with(['~', '/'])
}

fn looks_fresh(session: &Value) -> bool {
    let windows = session["windows"].as_array().map_or(0, Vec::len);
    let mut panes = panes(session);
    let (Some(pane), None) = (panes.next(), panes.next()) else {
        return false;
    };
    let records = pane["records"].as_array().is_some_and(|r| !r.is_empty());
    session["label"] != SESSION
        && windows == 1
        && !records
        && pane["process"]["name"].as_str().is_some_and(is_shell)
        && shell_title(pane["title"].as_str().unwrap_or(""))
}

fn describe(session: &Value) -> String {
    format!(
        "{}: {} tabs, {} panes",
        session["session_id"].as_str().unwrap_or("?"),
        session["windows"].as_array().map_or(0, Vec::len),
        panes(session).count()
    )
}

fn call(args: &Value) -> Result<Value> {
    let reply = rex::run_script(None, SCRIPT, args)?;
    jump::check(&reply, "window", "rexboard window")?;
    Ok(reply)
}

/// This host's sessions, from the snapshot script.
fn sessions() -> Result<Vec<Value>> {
    let snapshot = rex::run_script(None, rex::SNAPSHOT, &json!({ "host": "local" }))?;
    Ok(snapshot["hosts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|host| host["host"] == "local")
        .flat_map(|host| host["sessions"].as_array().into_iter().flatten().cloned())
        .collect())
}

/// The board's session: labelled `rexboard`, with a `board` tab whose pane runs the board (its
/// title is `board`, or its process is `rexboard`). Someone's own `rexboard` session never is.
fn board(sessions: &[Value]) -> Option<Board> {
    sessions.iter().find_map(|session| {
        if session["label"] != SESSION {
            return None;
        }
        let tab = session["windows"].as_array()?.iter().find(|w| {
            w["label"] == TAB
                && w["blocks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|b| b["title"] == TAB || b["process"]["name"] == SESSION)
        })?;
        Some(Board {
            id: session["session_id"].as_str()?.to_owned(),
            tab: tab["window_id"].as_str()?.to_owned(),
        })
    })
}

/// Makes the `rexboard` session with the board running in its `board` tab, and waits for the
/// board to start in it.
fn create() -> Result<Board> {
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
    let deadline = Instant::now() + APPEAR_WITHIN;
    loop {
        if let Some(board) = board(&sessions()?) {
            return Ok(board);
        }
        if Instant::now() >= deadline {
            return Err(Error::Rex(format!(
                "rex new {SESSION} made no session running the board within {}s",
                APPEAR_WITHIN.as_secs()
            )));
        }
        thread::sleep(POLL);
    }
}
