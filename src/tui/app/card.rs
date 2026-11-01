//! The card's state on the board: whether it shows, tui.json, and the tail it reads by itself.

use super::*;

impl App {
    /// Whether the card shows under the list: once `c` turned it on, and always while it holds the
    /// key table or a failed jump's reason, which only it shows whole. `pick` has none.
    pub fn card_shown(&self) -> bool {
        !self.pick && (self.card || self.keys || self.failed.is_some() || self.answer.is_some())
    }

    pub(super) fn prefs(&self) -> crate::tui::prefs::Prefs {
        crate::tui::prefs::Prefs {
            card: Some(self.card),
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

    /// A board with the card off, the way a new install starts.
    fn listed() -> App {
        let mut app = App::default();
        app.set_rows(rows(), hosts());
        app
    }

    #[test]
    fn the_list_fills_the_body_by_default_and_c_toggles_and_saves_and_p_does_nothing() {
        let mut app = listed();
        let text = render(&app, 100, 30);
        assert!(
            !text.contains("━━") && !text.contains("capturing"),
            "{text}"
        );
        for width in [80, 140, 200] {
            let text = render(&app, width, 30);
            assert!(
                !text.contains('─'),
                "no rule above the footer at {width}:\n{text}"
            );
        }
        let saved = |effect: Option<Effect>| match effect {
            Some(Effect::SavePrefs(prefs)) => prefs.card,
            other => panic!("{other:?}"),
        };
        assert_eq!(saved(press(&mut app, KeyCode::Char('c'))), Some(true));
        assert!(
            render(&app, 100, 30).contains("━━ ! blocked"),
            "the card shows"
        );
        assert_eq!(saved(press(&mut app, KeyCode::Char('c'))), Some(false));
        // The preview strip is gone: `p` is not a key, and nothing is read or saved.
        let before = render(&app, 100, 30);
        assert_eq!(press(&mut app, KeyCode::Char('p')), None, "no effect");
        assert_eq!(render(&app, 100, 30), before, "no strip, no change");
        assert!(app.toast().is_none(), "and no toast");
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
        app.tuning.tail_lines = Some(1000.into());
        assert_eq!(lines(app.tail()), 1000);
        assert_eq!(
            lines(press(&mut app, KeyCode::Char(' '))),
            1000,
            "the full view too"
        );
        app.peeked("mini/seat-b", Ok(vec![]));
        app.tuning.tail_lines = Some(5001.into());
        assert_eq!(
            lines(app.live(Instant::now() + Duration::from_secs(3600))),
            200,
            "out of range is the default"
        );
    }
}
