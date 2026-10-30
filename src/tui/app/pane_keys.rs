//! The keys that act on a row's pane: `enter` and `-` jump, `dd` closes after its question, `yy` and
//! `yp` copy.

use super::*;

impl App {
    /// `enter`: go to the current row's pane, one jump at a time.
    pub(super) fn jump(&mut self) -> Option<Effect> {
        if self.busy() {
            return None;
        }
        let row = self.current()?.clone();
        self.going = Some(format!(
            "going to {}…",
            path(&labels(&row, true), usize::MAX)
        ));
        Some(Effect::Jump(Box::new(row)))
    }

    /// Whether a jump or a close is still running, and if so says so.
    pub(super) fn busy(&mut self) -> bool {
        if self.going.is_some() {
            self.say("a jump or close is already running");
        }
        self.going.is_some()
    }

    /// The board's own pane, which it never answers or closes.
    pub(super) fn is_own(&self, row: &Row) -> bool {
        is_own(row, self.own.as_deref())
    }

    /// `-`: back to the pane before the last jump, as `cd -` does.
    pub(super) fn back(&mut self) -> Option<Effect> {
        if self.busy() {
            return None;
        }
        self.going = Some("going back…".into());
        Some(Effect::Back)
    }

    /// `dd`: ask before closing the current row's pane. Never the board's own.
    pub(super) fn ask_close(&mut self, now: Instant) {
        let Some(row) = self.current() else {
            return;
        };
        if self.is_own(row) || runs_rexboard(row) {
            self.failed = Some(RUNS_THE_BOARD.into());
            return;
        }
        self.closing = Some((row.clone(), now));
    }

    /// Where `row` is, as the list tells it apart from the rows that read the same (`#N`), in
    /// `max` columns: the path gives way, never the suffix.
    pub(super) fn place(&self, row: &Row, max: usize) -> String {
        let whole = path(&labels(row, false), usize::MAX);
        let at = self.rows.iter().position(|r| r.key == row.key);
        let full = at.map_or_else(
            || whole.clone(),
            |at| wheres(&self.rows, usize::MAX, false, true).swap_remove(at),
        );
        if full.width() <= max {
            return full;
        }
        // Both ends stay: the session, and the block that would close.
        let suffix = full.strip_prefix(whole.as_str()).unwrap_or_default();
        let room = max.saturating_sub(suffix.width()).max(1);
        format!("{}{suffix}", squeeze(&whole, room))
    }

    /// The footer while `dd` waits for `y`: the place as the list shows it, with its `#N`, and
    /// what closing it ends, in `width` columns. Only the place gives way.
    pub fn close_question(&self, width: usize) -> Option<String> {
        let (row, _) = self.closing.as_ref()?;
        let tmux = tmux_note(row);
        let full = self.place(row, usize::MAX);
        let ending = ending(row);
        // A narrow footer drops what it ends before it cuts the place.
        let rest = [ending.as_str(), ""]
            .into_iter()
            .map(|ending| format!(" on {}{ending}?{tmux} y closes", row.pane.host))
            .find(|rest| "close ".width() + full.width() + rest.width() <= width)
            .unwrap_or_else(|| format!(" on {}?{tmux} y closes", row.pane.host));
        let room = width.saturating_sub("close ".width() + rest.width());
        Some(format!("close {}{rest}", self.place(row, room)))
    }

    /// The key after `dd`'s question: only `y` closes, and only once the question has been up
    /// for GUARD, so typing or a paste that hits `dd` closes nothing.
    pub(super) fn answer_close(
        &mut self,
        row: Row,
        asked: Instant,
        key: K,
        now: Instant,
    ) -> Option<Effect> {
        if now.duration_since(asked) < GUARD {
            self.say("nothing closed (keys came too fast)");
            return None;
        }
        if key != keys::K(KeyCode::Char('y'), false) {
            self.say("nothing closed");
            return None;
        }
        if self.busy() {
            return None;
        }
        self.going = Some(format!("closing {}…", self.place(&row, usize::MAX)));
        Some(Effect::Close(Box::new(row)))
    }

    /// `yy` copies the row's message and `yp` where it is, as `session › window › block @host`,
    /// both without terminal control, so a paste cannot end early and run what follows.
    pub(super) fn copy(&mut self, location: bool) -> Option<Effect> {
        let row = self.current()?;
        let text = if location {
            clean(&format!(
                "{} @{}",
                path(&labels(row, false), usize::MAX),
                row.pane.host
            ))
        } else {
            clean(row.pane.msg.as_deref().unwrap_or_default())
                .trim()
                .to_owned()
        };
        if text.is_empty() {
            self.failed = Some("this row has no message to copy · yp copies where it is".into());
            return None;
        }
        let shown = if location {
            text.clone()
        } else {
            format!("\"{text}\"")
        };
        Some(Effect::Copy { text, shown })
    }
}

pub(crate) const RUNS_THE_BOARD: &str = "that pane runs the board; it stays open";

/// The pane whose REX_BLOCK is `own`, on this host: the board's, or the caller's for the CLI.
pub(crate) fn is_own(row: &Row, own: Option<&str>) -> bool {
    row.pane.host == "local" && own == Some(row.pane.block_id.as_str())
}

pub(crate) fn runs_rexboard(row: &Row) -> bool {
    row.pane.process.as_deref() == Some("rexboard")
}

/// What closing `row` ends, as `dd`'s question says it: `, ending claude (blocked · permission)`.
pub(crate) fn ending(row: &Row) -> String {
    let state = match (row.pane.state.as_str(), row.pane.kind.as_deref()) {
        ("blocked", Some(kind)) if !kind.is_empty() => format!(" (blocked · {kind})"),
        (state @ ("blocked" | "working"), _) => format!(" ({state})"),
        _ => String::new(),
    };
    format!(", ending {}{state}", harness(row))
}

/// Closing a tmux client leaves its session running, which the question says.
pub(crate) fn tmux_note(row: &Row) -> &'static str {
    if row.pane.in_tmux() {
        " (tmux keeps running)"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    #[test]
    fn the_tmux_note_covers_the_wrapper_around_tmux_with_or_without_a_status() {
        let mut row = rows().remove(0);
        assert_eq!(tmux_note(&row), "");
        row.pane.process = Some("bash".into());
        for source in ["bridge", "inferred"] {
            row.pane.source = source.into();
            row.pane.tmux = true;
            assert_eq!(tmux_note(&row), " (tmux keeps running)", "{source}");
        }
    }

    #[test]
    fn dd_asks_and_only_y_closes() {
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        typed(&mut app, "dd");
        assert_eq!(
            app.close_question(usize::MAX).as_deref(),
            Some("close api › main › seat-b on mini, ending claude (blocked)? y closes")
        );
        assert!(press(&mut app, KeyCode::Esc).is_none());
        assert!(app.close_question(usize::MAX).is_none());
        assert_eq!(app.toast(), Some("nothing closed"));
        typed(&mut app, "dd");
        assert!(
            press(&mut app, KeyCode::Char('d')).is_none(),
            "a third d is not y"
        );
        assert_eq!(app.toast(), Some("nothing closed"));
        typed(&mut app, "dd");
        assert!(matches!(
            press(&mut app, KeyCode::Char('y')),
            Some(Effect::Close(row)) if row.key == "mini/seat-b"
        ));
        assert_eq!(app.going.as_deref(), Some("closing api › main › seat-b…"));
        app.went(Ok("closed api › main › seat-b on mini".into()));
        assert_eq!(app.toast(), Some("closed api › main › seat-b on mini"));
    }

    /// `dd` in the middle of a word, as in "buddy", asks nothing, and a cancelled question stays
    /// cancelled whatever comes after a pause.
    #[test]
    fn dd_in_a_word_asks_nothing_and_a_cancel_stays() {
        for pause in 0.."buddy".len() {
            let mut app = app();
            let effects = typed_at(&mut app, "buddy", 120, Some(pause), IDLE);
            assert!(!effects.iter().any(acts_on_a_pane), "paused at {pause}");
        }
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut app = app();
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(0));
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(100));
        app.handle_at(KeyEvent::from(KeyCode::Char('x')), at(200));
        assert!(app.close_question(usize::MAX).is_none());
        assert!(
            app.handle_at(KeyEvent::from(KeyCode::Char('y')), at(900))
                .is_none(),
            "y after a pause does not bring the question back"
        );
    }

    #[test]
    fn dd_names_a_default_labelled_twin_as_the_list_does() {
        let mut app = app();
        let twin = |block: &str| {
            let mut row = row(&format!("local/{block}"), "working", "working", true);
            row.pane.pane = "claude".into();
            row.pane.process = Some("claude".into());
            row
        };
        app.set_rows(vec![twin("one"), twin("two")], hosts());
        let mut asked = Vec::new();
        for key in ["local/one", "local/two"] {
            app.selected = Some(Spot::Row(key.into()));
            typed(&mut app, "dd");
            asked.push(app.close_question(usize::MAX).unwrap());
            press(&mut app, KeyCode::Esc);
        }
        assert_ne!(asked[0], asked[1], "{asked:?}");
        assert!(asked[0].starts_with("close api › main #"), "{asked:?}");
    }

    #[test]
    fn dd_never_takes_the_boards_own_pane_and_forgets_a_row_that_left() {
        let mut app = app();
        app.own = Some("seat-a".into());
        app.selected = Some(Spot::Row("local/seat-a".into()));
        typed(&mut app, "dd");
        assert!(
            app.close_question(usize::MAX).is_none(),
            "the board's own pane"
        );
        assert_eq!(
            app.failed.as_deref(),
            Some("that pane runs the board; it stays open")
        );
        let mut rows = rows();
        rows[2].pane.process = Some("rexboard".into());
        rows[1].pane.process = Some("tmux".into());
        app.set_rows(rows.clone(), hosts());
        app.own = None;
        app.selected = Some(Spot::Row("mini/codex".into()));
        typed(&mut app, "dd");
        assert!(
            app.close_question(usize::MAX).is_none(),
            "a pane running rexboard"
        );
        app.selected = Some(Spot::Row("local/claude".into()));
        typed(&mut app, "dd");
        assert_eq!(
            app.close_question(usize::MAX).as_deref(),
            Some(
                "close api › main › claude on local, ending claude? (tmux keeps running) y closes"
            )
        );
        rows.remove(1);
        app.set_rows(rows, hosts());
        assert!(
            app.close_question(usize::MAX).is_none(),
            "its row left the board"
        );
        assert_eq!(
            app.toast(),
            Some("api › main › claude left the board; nothing to close")
        );
        let mut pick = super::testkit::app();
        pick.pick = true;
        typed(&mut pick, "dd");
        assert!(pick.close_question(usize::MAX).is_none(), "pick only picks");
    }

    #[test]
    fn yy_copies_the_message_and_yp_the_place_without_control() {
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        assert!(matches!(
            typed(&mut app, "yy").pop().flatten(),
            Some(Effect::Copy { text, shown }) if text == "seat-b msg" && shown == "\"seat-b msg\""
        ));
        assert!(matches!(
            typed(&mut app, "yp").pop().flatten(),
            Some(Effect::Copy { text, shown })
                if text == "api › main › seat-b @mini" && shown == text
        ));
        let mut rows = rows();
        rows[0].pane.msg = Some("done\x1b[201~echo hi\n".into());
        rows[0].pane.tab = "ma\x1b[31min".into();
        app.set_rows(rows.clone(), hosts());
        assert!(
            matches!(
                typed(&mut app, "yy").pop().flatten(),
                Some(Effect::Copy { text, .. }) if text == "doneecho hi"
            ),
            "a paste cannot end early"
        );
        assert!(matches!(
            typed(&mut app, "yp").pop().flatten(),
            Some(Effect::Copy { text, .. }) if text == "api › main › seat-b @mini"
        ));
        rows[0].pane.msg = None;
        app.set_rows(rows, hosts());
        assert!(typed(&mut app, "yy").pop().flatten().is_none());
        assert_eq!(
            app.failed.as_deref(),
            Some("this row has no message to copy · yp copies where it is")
        );
    }

    #[test]
    fn enter_says_where_it_goes_and_starts_one_jump_at_a_time() {
        let mut app = app();
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Jump(row)) if row.key == "mini/seat-b"
        ));
        assert_eq!(app.going.as_deref(), Some("going to api › seat-b…"));
        assert_eq!(press(&mut app, KeyCode::Enter), None, "no second jump");
        assert_eq!(app.toast(), Some("a jump or close is already running"));
        app.went(Err("the Rex app on studio did not answer within 5s".into()));
        assert_eq!(app.toast(), None, "the end of the jump clears it");
        assert_eq!(app.going, None);
        assert_eq!(
            app.failed.as_deref(),
            Some("the Rex app on studio did not answer within 5s")
        );
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.failed, None, "the next key clears it");
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Jump(_))
        ));
        app.went(Ok("jumped to api › claude on local".into()));
        assert_eq!(app.toast(), Some("jumped to api › claude on local"));
    }

    #[test]
    fn minus_goes_back_one_jump_at_a_time() {
        let mut app = app();
        assert!(matches!(
            press(&mut app, KeyCode::Char('-')),
            Some(Effect::Back)
        ));
        assert_eq!(app.going.as_deref(), Some("going back…"));
        assert!(press(&mut app, KeyCode::Char('-')).is_none());
        assert!(press(&mut app, KeyCode::Enter).is_none());
        assert_eq!(app.toast(), Some("a jump or close is already running"));
        app.went(Err("previous pane is gone".into()));
        assert_eq!(app.failed.as_deref(), Some("previous pane is gone"));
        let mut pick = super::testkit::app();
        pick.pick = true;
        assert!(
            press(&mut pick, KeyCode::Char('-')).is_none(),
            "pick only picks"
        );
    }
}
