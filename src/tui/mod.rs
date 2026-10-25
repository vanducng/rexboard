mod app;
mod theme;
mod view;

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;

use crate::client::{self, Rows};
use crate::error::{Error, Result};
use crate::output::clean;
use crate::{interrupted, rex, stop_on_signals};

use app::{App, Effect};

/// Peek shows this many lines from the end of the pane.
const PEEK_LINES: usize = 12;

enum Msg {
    Rows(Rows),
    Gone(String),
    Peek(String, std::result::Result<Vec<String>, String>),
    Say(String),
}

pub fn run() -> Result<()> {
    // SIGTERM or SIGHUP (the board window closing) ends the loop, so Restore puts the terminal back.
    stop_on_signals();
    watchdog();
    match board() {
        // The terminal is gone, so there is no one to tell; printing would only panic.
        Err(_) if unsafe { libc::isatty(0) } == 0 => Ok(()),
        other => other,
    }
}

fn board() -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let generation = Arc::new(AtomicU64::new(0));
    listen(&tx, &generation);
    let mut app = App::default();
    let mut terminal = ratatui::init();
    let _restore = Restore;
    let mut tick = Instant::now();
    loop {
        terminal.draw(|frame| view::draw(frame, &app, chrono::Utc::now()))?;
        let mut changed = false;
        while !changed {
            if interrupted() {
                return Ok(());
            }
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        match app.handle(key) {
                            Some(Effect::Quit) => return Ok(()),
                            Some(effect) => act(effect, &tx, &generation),
                            None => {}
                        }
                        changed = true;
                    }
                    Event::Resize(..) => changed = true,
                    _ => {}
                }
            }
            loop {
                match rx.recv_timeout(Duration::ZERO) {
                    Ok(msg) => {
                        apply(&mut app, msg);
                        changed = true;
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }
            if tick.elapsed() >= Duration::from_secs(1) {
                tick = Instant::now();
                changed = true;
            }
        }
    }
}

/// Once the terminal hangs up, crossterm's poll spins on EOF reads and never returns to the loop,
/// so a stop signal or a lost terminal would leave the board burning CPU. This thread ends the
/// process when the loop has not stopped within a second.
fn watchdog() {
    thread::spawn(|| {
        let mut asked: Option<Instant> = None;
        loop {
            thread::sleep(Duration::from_millis(250));
            let gone = unsafe { libc::isatty(0) } == 0;
            if interrupted() || gone {
                let since = *asked.get_or_insert_with(Instant::now);
                if gone || since.elapsed() >= Duration::from_secs(1) {
                    let _ = ratatui::try_restore();
                    std::process::exit(0);
                }
            }
        }
    });
}

fn apply(app: &mut App, msg: Msg) {
    match msg {
        Msg::Rows(rows) => app.set_rows(rows.rows, rows.hosts),
        Msg::Gone(why) => {
            app.connected = false;
            app.say(why);
        }
        Msg::Peek(key, lines) => {
            if let Some(peek) = app.peek.as_mut().filter(|peek| peek.key == key) {
                peek.lines = Some(lines);
            }
        }
        Msg::Say(text) => app.say(text),
    }
}

fn act(effect: Effect, tx: &Sender<Msg>, generation: &Arc<AtomicU64>) {
    let tx = tx.clone();
    match effect {
        Effect::Seen { key, label, state } => {
            thread::spawn(move || {
                let text = match client::call(&json!({ "cmd": "seen", "key": key })) {
                    Ok(_) => seen_message(&label, &state),
                    Err(err) => format!("seen failed: {err}"),
                };
                let _ = tx.send(Msg::Say(text));
            });
        }
        Effect::Peek { key, host, block } => {
            thread::spawn(move || {
                let lines = capture(&host, &block).map_err(|err| format!("peek failed: {err}"));
                let _ = tx.send(Msg::Peek(key, lines));
            });
        }
        Effect::Reconnect => listen(&tx, generation),
        Effect::Quit => {}
    }
}

/// Starts a subscriber thread. A newer `listen` makes the older one stop at its next tick.
fn listen(tx: &Sender<Msg>, generation: &Arc<AtomicU64>) {
    let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, generation) = (tx.clone(), generation.clone());
    thread::spawn(move || {
        let current = || generation.load(Ordering::SeqCst) == mine;
        let result = client::subscribe(|rows| {
            if let Some(rows) = rows
                && (!current() || tx.send(Msg::Rows(rows)).is_err())
            {
                return false;
            }
            current()
        });
        if current() {
            let why = match result {
                Err(Error::Serve { message, .. }) if message == "serve stopped" => {
                    "serve stopped".to_owned()
                }
                Err(err) => format!("serve: {err}"),
                Ok(()) => "serve stopped".to_owned(),
            };
            let _ = tx.send(Msg::Gone(format!("{why} · r reconnects")));
        }
    });
}

/// The last lines of a pane, as plain text with control characters removed.
fn capture(host: &str, block: &str) -> Result<Vec<String>> {
    let mut cmd = Command::new(rex::locate()?);
    if host != "local" {
        cmd.args(["-S", host]);
    }
    cmd.args(["capture", "-b", block, "--trim", "--unwrap"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = rex::spawn(&mut cmd)
        .and_then(std::process::Child::wait_with_output)
        .map_err(|err| Error::Rex(format!("rex capture did not run: {err}")))?;
    if !out.status.success() {
        return Err(Error::Rex(format!("rex capture failed ({})", out.status)));
    }
    Ok(peek_lines(&String::from_utf8_lossy(&out.stdout)))
}

/// The last lines of captured pane text, with every escape sequence and control character
/// removed: the text comes from whatever runs in that pane.
fn peek_lines(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(clean).collect();
    lines[lines.len().saturating_sub(PEEK_LINES)..].to_vec()
}

/// What `s` reports. A blocked or erroring pane stays in NEEDS YOU, so it says why.
fn seen_message(label: &str, state: &str) -> String {
    match state {
        "blocked" => format!("{label} marked seen; it stays in NEEDS YOU while blocked"),
        "error" => format!("{label} marked seen; it stays in NEEDS YOU while in error"),
        _ => format!("{label} marked seen"),
    }
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        // try_restore: restore() prints its error, which panics once the terminal is gone.
        let _ = ratatui::try_restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_lines_keep_the_last_twelve_without_escape_sequences() {
        let mut text: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        text.push_str("\x1b[31mRED\x1b[0m\n");
        text.push_str("clip \x1b]52;c;aGVsbG8=\x07end\n");
        text.push_str("title \x1b]0;set by the pane\x1b\\end\n");
        text.push_str("c1 \u{9b}2Jend\n");
        text.push_str("tab\there\r\n");
        let lines = peek_lines(&text);
        assert_eq!(lines.len(), PEEK_LINES);
        assert_eq!(lines.last().unwrap(), "tab here");
        assert!(
            lines.iter().all(|l| !l.chars().any(char::is_control)),
            "{lines:?}"
        );
        assert!(lines.contains(&"RED".to_owned()), "{lines:?}");
        assert!(lines.contains(&"clip end".to_owned()), "{lines:?}");
        assert!(lines.contains(&"title end".to_owned()), "{lines:?}");
        assert!(lines.contains(&"c1 end".to_owned()), "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| !l.contains("[31m") && !l.contains("]52;")),
            "{lines:?}"
        );
    }

    #[test]
    fn seen_says_when_a_row_stays_in_needs_you() {
        assert_eq!(seen_message("seat-a", "done"), "seat-a marked seen");
        assert_eq!(
            seen_message("seat-a", "blocked"),
            "seat-a marked seen; it stays in NEEDS YOU while blocked"
        );
    }
}
