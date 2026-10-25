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
use crate::rex;

use app::{App, Effect};

/// Peek shows this many lines from the end of the pane.
const PEEK_LINES: usize = 12;

enum Msg {
    Rows(Rows),
    Gone(String),
    Peek(String, Vec<String>),
    Say(String),
}

pub fn run() -> Result<()> {
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
        Effect::Seen(key) => {
            thread::spawn(move || {
                let text = match client::call(&json!({ "cmd": "seen", "key": key })) {
                    Ok(_) => format!("seen {key}"),
                    Err(err) => format!("seen failed: {err}"),
                };
                let _ = tx.send(Msg::Say(text));
            });
        }
        Effect::Peek { key, host, block } => {
            thread::spawn(move || {
                let lines = capture(&host, &block)
                    .unwrap_or_else(|err| vec![format!("peek failed: {err}")]);
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
            let why = result
                .err()
                .map_or("serve stopped".into(), |err| err.to_string());
            let _ = tx.send(Msg::Gone(format!("{why}; r reconnects")));
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
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<String> = text
        .lines()
        .map(|line| line.chars().filter(|c| !c.is_control()).collect())
        .collect();
    Ok(lines[lines.len().saturating_sub(PEEK_LINES)..].to_vec())
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
