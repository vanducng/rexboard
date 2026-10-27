pub(crate) mod answer;
mod app;
pub(crate) mod card;
mod clipboard;
pub(crate) mod columns;
mod effects;
mod full;
mod icons;
mod keys;
mod markdown;
mod meta;
mod prefs;
pub(crate) mod query;
pub(crate) mod screen;
mod status;
mod theme;
mod view;
mod views;
mod vt;

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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
use crate::{interrupted, jump, rex, send, stop_on_signals};

use app::{App, Effect, Link};
pub(crate) use app::{RUNS_THE_BOARD, ending, is_own, runs_rexboard, tmux_note};
use effects::{Msg, act, apply, listen, tui_json};
pub(crate) use effects::{screen as read_screen, tail};
use screen::Screen;

/// tui.json, or the defaults.
pub(crate) fn prefs() -> prefs::Prefs {
    tui_json()
        .map(|path| prefs::load(&path))
        .unwrap_or_default()
}

/// The board as tui.json, the environment and the pane it runs in set it up.
fn start_app(pick: bool) -> App {
    let mut app = App::default();
    app.pick = pick;
    let prefs = prefs();
    app.hidden = prefs.detail_hidden;
    app.motion = prefs.motion;
    app.icons = prefs.icons;
    app.card_height = prefs.card_height;
    app.columns = prefs.columns;
    app.nerd = match std::env::var("REXBOARD_ICONS").as_deref() {
        Ok("nerd") => true,
        Ok("text") => false,
        _ => prefs.icons == prefs::Icons::Nerd,
    };
    theme::set_motion(
        prefs.motion == prefs::Motion::Full
            && std::env::var("REXBOARD_REDUCED_MOTION").as_deref() != Ok("1"),
    );
    app.own = std::env::var("REX_BLOCK").ok().filter(|id| !id.is_empty());
    app
}

/// Applies every message waiting and acts on what they ask: whether any came, or None once the
/// senders are gone.
fn drain(
    app: &mut App,
    rx: &Receiver<Msg>,
    tx: &Sender<Msg>,
    generation: &Arc<AtomicU64>,
) -> Option<bool> {
    let mut changed = false;
    loop {
        match rx.recv_timeout(Duration::ZERO) {
            Ok(msg) => {
                if let Some(effect) = apply(app, msg) {
                    act(effect, tx, generation);
                }
                changed = true;
            }
            Err(RecvTimeoutError::Timeout) => return Some(changed),
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
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
    let mut app = start_app(pick);
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
            match drain(&mut app, &rx, &tx, &generation) {
                Some(true) => changed = true,
                Some(false) => {}
                None => return Ok(None),
            }
            let now = Instant::now();
            app.note_card(now);
            if let Some(effect) = app.auto_tail(now).or_else(|| app.live(now)) {
                act(effect, &tx, &generation);
                changed = true;
            }
            // An animated glyph on screen redraws at its own pace; otherwise once a second.
            if tick.elapsed() >= app.frame_every().unwrap_or(Duration::from_secs(1)) {
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
        MouseEventKind::ScrollUp => app.scroll(event.row, true),
        MouseEventKind::ScrollDown => app.scroll(event.row, false),
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

    #[test]
    fn mouse_capture_follows_the_table_view_and_never_reaches_pick() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = app::testkit::app();
        let mut on = false;
        let mut out = Vec::new();
        let mut step = |app: &App, out: &mut Vec<u8>| {
            out.clear();
            sync_mouse(out, app, &mut on);
            String::from_utf8_lossy(out).into_owned()
        };
        assert_eq!(step(&app, &mut out), "", "status: off, nothing written");
        app::testkit::press(&mut app, KeyCode::Char('v'));
        assert!(step(&app, &mut out).contains("\x1b[?1000h"), "table: on");
        assert_eq!(step(&app, &mut out), "", "only on a change");
        app::testkit::press(&mut app, KeyCode::Char('v'));
        assert!(step(&app, &mut out).contains("\x1b[?1000l"), "harness: off");
        let mut pick = app::testkit::app();
        pick.pick = true;
        pick.view = views::views().remove(1);
        assert!(!pick.clicks(), "pick keeps text selection");
    }

    #[test]
    fn motion_and_drags_ask_for_no_redraw() {
        use ratatui::crossterm::event::KeyModifiers;
        let mut app = app::testkit::app();
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
}
