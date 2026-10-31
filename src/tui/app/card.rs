//! The card's state on the board: whether it shows, tui.json, and the tail it reads by itself.

use super::*;

/// The preview strip's row, since when it has shown, when its tail was last asked for and whether
/// that read is still out.
pub(super) struct Strip {
    pub key: String,
    since: Instant,
    asked: Option<Instant>,
    pub reading: bool,
}

impl Strip {
    fn new(key: String, now: Instant) -> Self {
        Self {
            key,
            since: now,
            asked: None,
            reading: false,
        }
    }
}

impl App {
    /// Whether the card shows under the list: once `c` turned it on, and always while it holds the
    /// key table or a failed jump's reason, which only it shows whole. `pick` has none.
    pub fn card_shown(&self) -> bool {
        !self.pick && (self.card || self.keys || self.failed.is_some() || self.answer.is_some())
    }

    /// Whether the preview strip shows above the footer: `p` turned it on and the card, which
    /// wins, does not. `pick` has none.
    pub fn strip_shown(&self) -> bool {
        !self.pick && self.preview && !self.card_shown() && self.full.is_none()
    }

    pub(super) fn prefs(&self) -> crate::tui::prefs::Prefs {
        crate::tui::prefs::Prefs {
            card: Some(self.card),
            preview: Some(self.preview),
            tuning: self.tuning.clone(),
            motion: self.motion,
            icons: self.icons,
            card_height: self.card_height,
            columns: self.columns.clone(),
            widths: self.widths.clone(),
        }
    }

    /// Notes which row's card shows from `now`, and lets go of a scroll once another row's
    /// does; the board calls it after every change.
    pub fn note_card(&mut self, now: Instant) {
        let key = self
            .card_shown()
            .then(|| self.current().map(|row| row.key.clone()))
            .flatten();
        match key {
            Some(key) if self.shown.as_ref().is_some_and(|(shown, _)| *shown == key) => {}
            Some(key) => self.shown = Some((key, now)),
            None => self.shown = None,
        }
        if self.card_scroll.as_ref().map(|(key, _)| key) != self.shown.as_ref().map(|(key, _)| key)
        {
            self.card_scroll = None;
        }
        let strip = self
            .strip_shown()
            .then(|| self.current().map(|row| row.key.clone()))
            .flatten();
        match strip {
            Some(key) if self.strip.as_ref().is_some_and(|s| s.key == key) => {}
            Some(key) => self.strip = Some(Strip::new(key, now)),
            None => self.strip = None,
        }
    }

    /// The strip's next read: the selected row's tail, once it has stayed selected for SETTLE and
    /// then every `preview_every` seconds, never while the last read is out. Nothing is read while
    /// the strip is hidden. The lines it has stay up between reads.
    pub fn preview_read(&mut self, now: Instant) -> Option<Effect> {
        let every = Duration::from_secs(self.tuning.every());
        let strip = self.strip.as_mut()?;
        let due = match strip.asked {
            None => now.duration_since(strip.since) >= SETTLE,
            Some(at) => !strip.reading && now.duration_since(at) >= every,
        };
        if !due {
            return None;
        }
        strip.asked = Some(now);
        strip.reading = true;
        let key = strip.key.clone();
        let row = self.rows.iter().find(|row| row.key == key)?;
        if self.peek.as_ref().is_none_or(|peek| peek.key != key) {
            self.peek = Some(Peek {
                key,
                lines: None,
                at: None,
                updated_at: row.pane.updated_at.clone(),
            });
        }
        Some(peek(row, false, self.tuning.lines()))
    }

    /// A tail for a blocked row whose card has shown for SETTLE, once per report.
    pub fn auto_tail(&mut self, now: Instant) -> Option<Effect> {
        let (key, since) = self
            .shown
            .as_ref()
            .filter(|_| self.answer.is_none() && self.full.is_none())?;
        let row = self.current().filter(|row| row.key == *key)?;
        if now.duration_since(*since) < SETTLE || row.pane.state != "blocked" {
            return None;
        }
        let want = (row.key.clone(), row.pane.updated_at.clone());
        if self.tailed.as_ref() == Some(&want) {
            return None;
        }
        self.tailed = Some(want);
        self.tail()
    }

    /// ctrl-e and ctrl-y: the card's text a line down or up from where the last draw left it.
    pub(super) fn card_scroll(&mut self, by: isize) {
        let drawn = self.card_drawn.get();
        if drawn.last == 0 {
            return self.say("nothing more to scroll");
        }
        if let Some(row) = self.current() {
            let at = drawn.at.saturating_add_signed(by).min(drawn.last);
            self.card_scroll = Some((row.key.clone(), at));
        }
    }

    /// Asks for the current row's tail; the card shows it when it comes.
    pub(super) fn tail(&mut self) -> Option<Effect> {
        let row = self.current()?;
        let effect = peek(row, self.full.is_some(), self.tuning.tail());
        self.peek = Some(Peek {
            key: row.key.clone(),
            lines: None,
            at: None,
            updated_at: row.pane.updated_at.clone(),
        });
        Some(effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    /// A board with the card and the strip off, the way a new install starts.
    fn listed() -> App {
        let mut app = App::default();
        app.set_rows(rows(), hosts());
        app
    }

    const T: Duration = Duration::from_millis(1);

    #[test]
    fn the_list_fills_the_body_by_default_and_c_and_p_each_toggle_and_save() {
        let mut app = listed();
        let text = render(&app, 100, 30);
        assert!(
            !text.contains("━━") && !text.contains("capturing"),
            "{text}"
        );
        let saved = |effect: Option<Effect>| match effect {
            Some(Effect::SavePrefs(prefs)) => (prefs.card, prefs.preview),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            saved(press(&mut app, KeyCode::Char('c'))),
            (Some(true), Some(false))
        );
        assert!(
            render(&app, 100, 30).contains("━━ ! blocked"),
            "the card shows"
        );
        assert_eq!(
            saved(press(&mut app, KeyCode::Char('c'))),
            (Some(false), Some(false))
        );
        assert_eq!(
            saved(press(&mut app, KeyCode::Char('p'))),
            (Some(false), Some(true))
        );
        assert!(app.strip_shown() && !app.card_shown());
        assert_eq!(
            saved(press(&mut app, KeyCode::Char('p'))),
            (Some(false), Some(false))
        );
    }

    #[test]
    fn the_strip_is_a_bar_and_the_selected_rows_last_lines_and_the_card_wins() {
        let mut app = listed();
        press(&mut app, KeyCode::Char('p'));
        app.peeked(
            "mini/seat-b",
            Ok((1..=9).map(|n| format!("out {n}")).collect()),
        );
        // `peeked` needs the read it answers.
        let key = "mini/seat-b".to_owned();
        app.peek = Some(Peek {
            key: key.clone(),
            lines: Some(Ok((1..=9).map(|n| format!("out {n}")).collect())),
            at: None,
            updated_at: None,
        });
        let text = render(&app, 100, 30);
        let lines: Vec<&str> = text.lines().collect();
        let bar = lines
            .iter()
            .position(|l| l.starts_with("━━ ! blocked"))
            .expect(&text);
        assert_eq!(
            lines[bar + 1..lines.len() - 1].len(),
            4,
            "the default 4 lines:\n{text}"
        );
        assert!(
            lines[bar + 1].contains("out 6") && lines[bar + 4].contains("out 9"),
            "{text}"
        );
        assert!(lines.last().unwrap().contains("p preview"), "{text}");
        let wide = render(&app, 200, 30);
        assert!(
            !wide.lines().last().unwrap().contains("scroll card"),
            "{wide}"
        );
        app.tuning.preview_lines = Some(2);
        let text = render(&app, 100, 30);
        assert_eq!(
            text.lines().filter(|l| l.contains("│ out")).count(),
            2,
            "{text}"
        );
        // With the card on, the card is what shows, and `p` says why.
        press(&mut app, KeyCode::Char('c'));
        assert!(!app.strip_shown());
        let text = render(&app, 100, 30);
        assert!(
            text.contains("━━ ! blocked") && text.contains("title"),
            "{text}"
        );
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(app.toast(), Some(BEHIND_CARD));
        // Turning the card off brings the strip back.
        press(&mut app, KeyCode::Char('c'));
        assert!(app.strip_shown());
    }

    #[test]
    fn the_strip_reads_after_it_settles_then_every_preview_every_and_never_while_hidden() {
        let mut app = listed();
        let t0 = Instant::now();
        for secs in [0, 1, 5, 60] {
            app.note_card(t0 + Duration::from_secs(secs));
            assert_eq!(
                app.preview_read(t0 + Duration::from_secs(secs)),
                None,
                "hidden: no read at {secs}s"
            );
        }
        press(&mut app, KeyCode::Char('p'));
        app.note_card(t0);
        assert_eq!(app.preview_read(t0), None, "a row passing by is not read");
        let read = app.preview_read(t0 + SETTLE + T);
        assert!(
            matches!(&read, Some(Effect::Peek { key, vt: false, lines: 4, .. }) if key == "mini/seat-b"),
            "{read:?}"
        );
        let asked = t0 + SETTLE + T;
        assert_eq!(
            app.preview_read(asked + Duration::from_secs(5)),
            None,
            "its read is still out"
        );
        app.peeked("mini/seat-b", Ok(vec!["one".into()]));
        assert_eq!(
            app.preview_read(asked + Duration::from_millis(1999)),
            None,
            "2 s by default"
        );
        assert!(matches!(
            app.preview_read(asked + Duration::from_secs(2)),
            Some(Effect::Peek { .. })
        ));
        app.peeked("mini/seat-b", Ok(vec!["two".into()]));
        app.tuning.preview_every = Some(5);
        let again = asked + Duration::from_secs(2);
        assert_eq!(app.preview_read(again + Duration::from_secs(4)), None);
        assert!(
            app.preview_read(again + Duration::from_secs(5)).is_some(),
            "preview_every holds"
        );
        // It follows the selection: another row is read once it has stayed selected.
        app.peeked("mini/seat-b", Ok(vec!["three".into()]));
        let moved = again + Duration::from_secs(10);
        press(&mut app, KeyCode::Char('j'));
        app.note_card(moved);
        assert_eq!(app.preview_read(moved), None);
        let read = app.preview_read(moved + SETTLE + T);
        assert!(
            matches!(&read, Some(Effect::Peek { key, .. }) if key == "local/claude"),
            "{read:?}"
        );
        // Hidden again: nothing more is read, however long it waits.
        press(&mut app, KeyCode::Char('p'));
        app.note_card(moved + Duration::from_secs(1));
        assert_eq!(app.preview_read(moved + Duration::from_secs(600)), None);
        // And the card, which wins, reads nothing of the strip's either.
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('c'));
        app.note_card(moved + Duration::from_secs(700));
        assert_eq!(app.preview_read(moved + Duration::from_secs(900)), None);
    }

    #[test]
    fn the_card_and_the_full_view_ask_for_tail_lines() {
        let mut app = listed();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        let lines = |effect: Option<Effect>| match effect {
            Some(Effect::Peek { lines, .. }) => lines,
            other => panic!("{other:?}"),
        };
        assert_eq!(lines(app.tail()), 200, "the default");
        app.tuning.tail_lines = Some(1000);
        assert_eq!(lines(app.tail()), 1000);
        assert_eq!(
            lines(press(&mut app, KeyCode::Char(' '))),
            1000,
            "the full view too"
        );
        app.peeked("mini/seat-b", Ok(vec![]));
        app.tuning.tail_lines = Some(5001);
        assert_eq!(
            lines(app.live(Instant::now() + Duration::from_secs(3600))),
            200,
            "out of range is the default"
        );
    }
}
