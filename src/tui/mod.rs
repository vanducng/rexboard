mod app;
mod card;
mod columns;
mod prefs;
mod status;
mod theme;
mod view;
mod views;

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, Event, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use serde_json::json;

use crate::board::Row;
use crate::client::{self, Rows};
use crate::error::{Error, Result};
use crate::output::{clean, labels, path};
use crate::state::Paths;
use crate::{interrupted, jump, rex, stop_on_signals};

use app::{App, Effect, Link};

/// Peek keeps this many lines from the end of the pane, enough for a 200-row pane; each draw shows
/// the newest that fit, so a resize needs no new capture.
const PEEK_LINES: usize = 200;
/// A peek reads at most this much of the end of a pane, and gives up after PEEK_WITHIN.
const PEEK_BYTES: usize = 48 * 1024;
const PEEK_WITHIN: Duration = Duration::from_secs(3);
/// Each peek takes the next number; one whose number is no longer the latest stops.
static PEEK_GEN: AtomicU64 = AtomicU64::new(0);

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
    // Before the subscriber thread, which may restart serve and say so.
    client::QUIET.store(true, Ordering::Relaxed);
    listen(&tx, &generation);
    let mut app = App::default();
    app.pick = pick;
    let prefs = tui_json()
        .map(|path| prefs::load(&path))
        .unwrap_or_default();
    app.hidden = prefs.detail_hidden;
    let mut terminal = ratatui::init();
    let _restore = Restore;
    let mut tick = Instant::now();
    let mut captured = false;
    loop {
        let size = terminal.size().unwrap_or_default();
        (app.width, app.height) = (size.width, size.height);
        sync_mouse(&mut std::io::stdout(), &app, &mut captured);
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
                    Event::Mouse(event) => changed = mouse(&mut app, event),
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
            let now = Instant::now();
            app.note_card(now);
            if let Some(effect) = app.auto_tail(now) {
                act(effect, &tx, &generation);
                changed = true;
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
    let _ = mouse_capture(&mut std::io::stdout(), false);
    let _ = ratatui::try_restore();
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
                peek.at = Some(chrono::Utc::now());
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
        Effect::Peek {
            key,
            host,
            session,
            block,
        } => {
            let mine = PEEK_GEN.fetch_add(1, Ordering::SeqCst) + 1;
            thread::spawn(move || {
                let current = || PEEK_GEN.load(Ordering::SeqCst) == mine;
                let lines = match rex::locate() {
                    Ok(rex) => peek(&rex, &host, &session, &block, &|| !current()),
                    Err(err) => Some(Err(format!("peek failed: {err}"))),
                };
                if let Some(lines) = lines.filter(|_| current()) {
                    let _ = tx.send(Msg::Peek(key, lines));
                }
            });
        }
        Effect::SavePrefs(prefs) => {
            let saved = tui_json()
                .ok_or_else(|| "no state dir".to_owned())
                .and_then(|path| {
                    prefs::save(&path, &prefs).map_err(|err| format!("{}: {err}", path.display()))
                });
            if let Err(why) = saved {
                let _ = tx.send(Msg::Say(format!("could not keep the card setting: {why}")));
            }
        }
        Effect::Reconnect => listen(&tx, generation),
        Effect::Quit => {}
    }
}

fn tui_json() -> Option<std::path::PathBuf> {
    Paths::find().ok().map(|paths| paths.tui())
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

/// What a peek shows: the pane's last lines, or why there are none. None when a newer peek
/// cancelled it, which then owns the slot.
fn peek(
    rex: &std::path::Path,
    host: &str,
    session: &str,
    block: &str,
    cancelled: &dyn Fn() -> bool,
) -> Option<std::result::Result<Vec<String>, String>> {
    match tail(rex, host, session, block, cancelled) {
        Ok(lines) => Some(Ok(lines)),
        Err(Error::Cancelled) => None,
        Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::TimedOut => Some(Err(format!(
            "peek gave up after {}s",
            PEEK_WITHIN.as_secs()
        ))),
        Err(err) => Some(Err(format!("peek failed: {err}"))),
    }
}

/// The last lines of a pane, as plain text with control characters removed: only the end of its
/// scrollback is read (`tail.lua`), within PEEK_WITHIN, and `cancelled` stops it early.
fn tail(
    rex: &std::path::Path,
    host: &str,
    session: &str,
    block: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<String>> {
    let args = json!({ "session_id": session, "block_id": block, "max_bytes": PEEK_BYTES });
    let remote = (host != "local").then_some(host);
    let reply = rex::run_script_with(rex, remote, rex::TAIL, &args, PEEK_WITHIN, cancelled)?;
    if reply["ok"] != true {
        let field = |name: &str| reply[name].as_str().unwrap_or("unknown").to_owned();
        return Err(Error::Rex(format!("{}: {}", field("step"), field("error"))));
    }
    Ok(peek_lines(reply["text"].as_str().unwrap_or_default()))
}

/// The last lines of captured pane text, with every escape sequence and control character
/// removed: the text comes from whatever runs in that pane. Trailing blanks go too, so a padded
/// line does not wrap into empty rows.
fn peek_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| clean(line).trim_end().to_owned())
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
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

/// Mouse capture stops the terminal's own text selection, so it is on only while clicks sort.
fn sync_mouse(out: &mut impl std::io::Write, app: &App, on: &mut bool) {
    if app.clicks() != *on {
        *on = app.clicks();
        let _ = mouse_capture(out, *on);
    }
}

/// On: button presses and the wheel (1000) in SGR form (1006), never motion, which would wake the
/// board on every pointer move. Off: everything any capture could have asked for.
fn mouse_capture(out: &mut impl std::io::Write, on: bool) -> std::io::Result<()> {
    if on {
        out.write_all(b"\x1b[?1000h\x1b[?1006h")?;
        out.flush()
    } else {
        ratatui::crossterm::execute!(out, DisableMouseCapture)
    }
}

/// A mouse event; true when it may have changed what is drawn.
fn mouse(app: &mut App, event: MouseEvent) -> bool {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => app.click(event.column, event.row),
        MouseEventKind::ScrollUp => app.scroll(true),
        MouseEventKind::ScrollDown => app.scroll(false),
        _ => return false,
    }
    true
}

/// Puts the terminal back on the way out, panics included (they unwind through it): mouse
/// capture off, whether or not it was on, then raw mode and the screen.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = mouse_capture(&mut std::io::stdout(), false);
        // try_restore: restore() prints its error, which panics once the terminal is gone.
        let _ = ratatui::try_restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rex that logs its argv and pid to `dir`, sleeps `sleep` seconds, then prints `reply`.
    fn fake_rex(
        dir: &std::path::Path,
        reply: &serde_json::Value,
        sleep: u32,
    ) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("reply"), reply.to_string()).unwrap();
        let rex = dir.join("rex");
        let d = dir.display();
        std::fs::write(
            &rex,
            format!(
                "#!/bin/sh\necho \"$@\" > '{d}/argv'\necho $$ > '{d}/pid'\ncat > /dev/null\nsleep {sleep}\ncat '{d}/reply'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&rex, std::fs::Permissions::from_mode(0o755)).unwrap();
        rex
    }

    fn gone(dir: &std::path::Path) -> bool {
        let pid: i32 = std::fs::read_to_string(dir.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        unsafe { libc::kill(pid, 0) != 0 }
    }

    #[test]
    fn a_peek_reads_the_tail_of_the_pane_on_its_own_host() {
        let dir = tempfile::tempdir().unwrap();
        let mut text: String = (1..=250).map(|n| format!("line {n}\n")).collect();
        text.push_str("\x1b[31mRED\x1b[0m\n\n  \n");
        let rex = fake_rex(
            dir.path(),
            &json!({ "ok": true, "text": text, "total": 9000, "cut": true }),
            0,
        );
        let lines = peek(&rex, "mini", "session:1", "block:2", &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(lines.len(), PEEK_LINES);
        assert_eq!(lines.last().unwrap(), "RED", "trailing blank lines dropped");
        assert!(
            lines.iter().all(|l| !l.chars().any(char::is_control)),
            "{lines:?}"
        );
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        let (head, args) = argv.trim().split_once(" --args ").unwrap();
        assert_eq!(head, "-S mini do - --json");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(args).unwrap(),
            json!({ "session_id": "session:1", "block_id": "block:2", "max_bytes": 49152 })
        );
        peek(&rex, "local", "session:1", "block:2", &|| false);
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        assert!(
            argv.starts_with("do - --json --args "),
            "no -S for this host: {argv}"
        );
    }

    #[test]
    fn a_failed_tail_says_which_step() {
        let dir = tempfile::tempdir().unwrap();
        let reply = json!({ "ok": false, "step": "blob_read", "error": "not_found" });
        let rex = fake_rex(dir.path(), &reply, 0);
        assert_eq!(
            peek(&rex, "local", "s", "b", &|| false),
            Some(Err("peek failed: blob_read: not_found".to_owned()))
        );
    }

    #[test]
    fn a_slow_host_gives_up_after_3s_and_its_rex_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true, "text": "late" }), 5);
        let started = std::time::Instant::now();
        let got = peek(&rex, "mini", "s", "b", &|| false);
        let took = started.elapsed();
        assert_eq!(got, Some(Err("peek gave up after 3s".to_owned())));
        assert!(
            took >= PEEK_WITHIN && took < Duration::from_secs(4),
            "{took:?}"
        );
        assert!(gone(dir.path()), "rex was killed");
    }

    #[test]
    fn a_newer_peek_cancels_the_older_one_and_its_rex_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true, "text": "stale" }), 5);
        let started = std::time::Instant::now();
        let newer = std::sync::atomic::AtomicBool::new(false);
        let got = thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(200));
                newer.store(true, Ordering::SeqCst);
            });
            peek(&rex, "local", "s", "b", &|| newer.load(Ordering::SeqCst))
        });
        assert_eq!(got, None, "nothing to apply");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "stopped well before the limit"
        );
        assert!(gone(dir.path()), "rex was killed");
    }

    #[test]
    fn mouse_capture_follows_the_table_view_and_never_reaches_pick() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = app::tests::app();
        let mut on = false;
        let mut out = Vec::new();
        let mut step = |app: &App, out: &mut Vec<u8>| {
            out.clear();
            sync_mouse(out, app, &mut on);
            String::from_utf8_lossy(out).into_owned()
        };
        assert_eq!(step(&app, &mut out), "", "status: off, nothing written");
        app::tests::press(&mut app, KeyCode::Char('v'));
        assert!(step(&app, &mut out).contains("\x1b[?1000h"), "table: on");
        assert_eq!(step(&app, &mut out), "", "only on a change");
        app::tests::press(&mut app, KeyCode::Char('v'));
        assert!(step(&app, &mut out).contains("\x1b[?1000l"), "harness: off");
        let mut pick = app::tests::app();
        pick.pick = true;
        pick.view = views::views().remove(1);
        assert!(!pick.clicks(), "pick keeps text selection");
    }

    #[test]
    fn motion_and_drags_ask_for_no_redraw() {
        use ratatui::crossterm::event::KeyModifiers;
        let mut app = app::tests::app();
        app.view = views::views().remove(1);
        let event = |kind| MouseEvent {
            kind,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(!mouse(&mut app, event(kind)), "{kind:?}");
        }
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
        ] {
            assert!(mouse(&mut app, event(kind)), "{kind:?}");
        }
    }

    #[test]
    fn capture_asks_for_buttons_only_and_off_clears_every_mode() {
        let mut on = Vec::new();
        mouse_capture(&mut on, true).unwrap();
        assert_eq!(on, b"\x1b[?1000h\x1b[?1006h", "no motion (1002, 1003)");
        let mut off = Vec::new();
        mouse_capture(&mut off, false).unwrap();
        let off = String::from_utf8_lossy(&off);
        for mode in ["1000", "1002", "1003", "1006", "1015"] {
            assert!(off.contains(&format!("\x1b[?{mode}l")), "{mode}: {off:?}");
        }
    }

    #[test]
    fn restoring_turns_mouse_capture_off() {
        let mut out = Vec::new();
        mouse_capture(&mut out, false).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("\x1b[?1000l") && text.contains("\x1b[?1006l"),
            "{text:?}"
        );
    }

    #[test]
    fn a_line_padded_with_blanks_takes_one_row() {
        let lines = peek_lines(&format!("foo{}\n", " ".repeat(120)));
        assert_eq!(lines, ["foo"]);
        assert_eq!(
            crate::output::chop(&lines[0], 45).len(),
            1,
            "one row at 45 columns"
        );
    }

    #[test]
    fn peek_lines_keep_the_last_200_without_escape_sequences() {
        let mut text: String = (1..=250).map(|n| format!("line {n}\n")).collect();
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
