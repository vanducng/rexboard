//! The card's state on the board: whether it shows, tui.json, and the tail it reads by itself.

use super::*;

impl App {
    /// Whether the card shows under the list: unless `c` hid it, and always while it holds the
    /// key table or a failed jump's reason, which only it shows whole. `pick` has none.
    pub fn card_shown(&self) -> bool {
        !self.pick && (!self.hidden || self.keys || self.failed.is_some() || self.answer.is_some())
    }

    pub(super) fn prefs(&self) -> crate::tui::prefs::Prefs {
        crate::tui::prefs::Prefs {
            detail_hidden: self.hidden,
            motion: self.motion,
            icons: self.icons,
            card_height: self.card_height,
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
        let effect = peek(row, self.full.is_some());
        self.peek = Some(Peek {
            key: row.key.clone(),
            lines: None,
            at: None,
            updated_at: row.pane.updated_at.clone(),
        });
        Some(effect)
    }
}
