mod app;
mod theme;
mod view;

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;

use crate::board::Row;
use crate::client::{self, Rows};
use crate::error::{Error, Result};
use crate::output::{clean, labels, path};
use crate::{interrupted, jump, rex, stop_on_signals};

use app::{App, Effect, Link};

/// Peek shows this many lines from the end of the pane.
const PEEK_LINES: usize = 12;

enum Msg {
    Rows(Rows),
    Gone(String),
    Peek(String, std::result::Result<Vec<String>, String>),
    Say(String),
    Jumped(std::result::Result<String, String>),
}

/// What the process exits with when a signal or a lost terminal ends it.
static LEAVE_CODE: AtomicI32 = AtomicI32::new(0);

pub fn run() -> Result<()> {
    // SIGTERM or SIGHUP (the board window closing) stops the loop, which puts the terminal back.
    stop_on_signals();
    watchdog();
    board(false).map(drop)
}

/// `rexboard pick`: the list alone. Enter leaves it and goes to the row; esc gives up.
pub fn pick(json: bool) -> Result<()> {
    LEAVE_CODE.store(1, Ordering::SeqCst);
    stop_on_signals();
    watchdog();
    let row = board(true)?.ok_or(Error::Cancelled)?;
    jump::go(&row)?;
    jump::done(&row, json);
    Ok(())
}

/// Runs the board until it quits, or until a pick chooses a row.
fn board(pick: bool) -> Result<Option<Row>> {
    let (tx, rx) = mpsc::channel();
    let generation = Arc::new(AtomicU64::new(0));
    listen(&tx, &generation);
    let mut app = App::default();
    app.pick = pick;
    let mut terminal = ratatui::init();
    let _restore = Restore;
    if !pick {
        title_board();
    }
    let mut tick = Instant::now();
    loop {
        terminal
            .draw(|frame| view::draw(frame, &app, chrono::Utc::now()))
            .unwrap_or_else(|_| leave());
        let mut changed = false;
        while !changed {
            if interrupted() {
                leave();
            }
            if event::poll(Duration::from_millis(50)).unwrap_or_else(|_| leave()) {
                match event::read().unwrap_or_else(|_| leave()) {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        match app.handle(key) {
                            Some(Effect::Quit) => return Ok(None),
                            Some(Effect::Jump(row)) if pick => return Ok(Some(*row)),
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
                    Err(RecvTimeoutError::Disconnected) => return Ok(None),
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
/// process when the loop has not stopped within a second, or at once when the terminal is gone.
fn watchdog() {
    thread::spawn(|| {
        let mut asked: Option<Instant> = None;
        loop {
            thread::sleep(Duration::from_millis(250));
            if terminal_gone()
                || interrupted()
                    && asked.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(1)
            {
                leave();
            }
        }
    });
}

/// isatty stays true on macOS after the other end closes, so ask poll for a hang-up instead.
/// macOS reports POLLHUP only when some event is asked for; nothing is read here.
fn terminal_gone() -> bool {
    let mut fd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    (unsafe { libc::poll(&raw mut fd, 1, 0) }) > 0
        && fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

/// Restores the terminal and exits without running destructors: ratatui's Terminal prints on
/// drop when it cannot show the cursor, and printing to a terminal that is gone panics inside
/// the panic hook, which aborts. A signal can come just before its terminal goes.
fn leave() -> ! {
    let _ = ratatui::try_restore();
    untitle();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::cursor::Show);
    std::process::exit(LEAVE_CODE.load(Ordering::SeqCst))
}

fn apply(app: &mut App, msg: Msg) {
    match msg {
        Msg::Rows(rows) => app.set_rows(rows.rows, rows.hosts),
        Msg::Gone(why) => {
            app.say(format!("{why} · r reconnects"));
            app.link = Link::Lost(why);
        }
        Msg::Peek(key, lines) => {
            if let Some(peek) = app.peek.as_mut().filter(|peek| peek.key == key) {
                peek.lines = Some(lines);
            }
        }
        Msg::Say(text) => app.say(text),
        Msg::Jumped(result) => app.jumped(result),
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
        Effect::SeenAll(keys) => {
            thread::spawn(move || {
                let mut marked = 0;
                let mut failed = None;
                for key in keys {
                    match client::call(&json!({ "cmd": "seen", "key": key })) {
                        Ok(_) => marked += 1,
                        Err(err) => failed = Some(err),
                    }
                }
                let rows = if marked == 1 { "row" } else { "rows" };
                let text = match failed {
                    None => format!("{marked} done {rows} marked seen"),
                    Some(err) => format!("{marked} done {rows} marked seen; seen failed: {err}"),
                };
                let _ = tx.send(Msg::Say(text));
            });
        }
        Effect::Jump(row) => {
            thread::spawn(move || {
                let result = jump::go(&row)
                    .map(|()| format!("jumped to {}", path(&labels(&row, true), usize::MAX)))
                    .map_err(|err| err.to_string());
                let _ = tx.send(Msg::Jumped(result));
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
            let _ = tx.send(Msg::Gone(why));
        }
    });
}

/// The last lines of a pane, as plain text with control characters removed.
fn capture(host: &str, block: &str) -> Result<Vec<String>> {
    let mut cmd = rex::command(&rex::locate()?);
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
        untitle();
    }
}

static TITLED: AtomicBool = AtomicBool::new(false);

/// Titles the pane `board`, so the Rex app titles the board's window `board - rexboard`. The
/// terminal's own title is pushed first and popped on the way out.
fn title_board() {
    use std::io::Write;
    let mut out = std::io::stdout();
    let set = write!(out, "\x1b[22;0t\x1b]2;{}\x07", crate::window::TAB).and_then(|()| out.flush());
    TITLED.store(set.is_ok(), Ordering::SeqCst);
}

fn untitle() {
    use std::io::Write;
    if TITLED.swap(false, Ordering::SeqCst) {
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[23;0t").and_then(|()| out.flush());
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
