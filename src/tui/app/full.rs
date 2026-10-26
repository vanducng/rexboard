//! The full view's state on the board: opening it, its live reads, and its keys.

use super::*;

impl App {
    /// `space`: the full view of the current row, with its first read.
    pub(super) fn open_full(&mut self, now: Instant) -> Option<Effect> {
        let key = self.current()?.key.clone();
        self.keys = false;
        self.full = Some(Full::new(key, now));
        self.tail()
    }

    /// The open view's next read, once the last is back and EVERY has passed. The lines it has
    /// stay up meanwhile, so the screen never blanks between reads.
    pub fn live(&mut self, now: Instant) -> Option<Effect> {
        let full = self.full.as_mut().filter(|full| full.due(now))?;
        let row = self.rows.iter().find(|row| row.key == full.key)?;
        full.asked = now;
        full.reading = true;
        Some(peek(row, true))
    }

    /// A read came back. While the view is open a failed read keeps the lines it had and says
    /// why in the bar.
    pub fn peeked(&mut self, key: &str, lines: Result<Vec<String>, String>) {
        let Some(peek) = self.peek.as_mut().filter(|peek| peek.key == key) else {
            return;
        };
        if let Some(full) = self.full.as_mut().filter(|full| full.key == key) {
            full.reading = false;
            full.error = lines.as_ref().err().cloned();
            if full.error.is_some() && matches!(peek.lines, Some(Ok(_))) {
                return;
            }
        }
        peek.lines = Some(lines);
        peek.at = Some(chrono::Utc::now());
    }

    /// The lines the open view shows, and how many fit between its bar and footer.
    pub(super) fn full_page(&self) -> (Vec<String>, usize) {
        let lines = self
            .full
            .as_ref()
            .map(|full| crate::tui::full::lines(full, self.peek.as_ref()).to_vec())
            .unwrap_or_default();
        (
            lines,
            usize::from(self.height.saturating_sub(2 + crate::tui::view::FOOTER_GAP)).max(1),
        )
    }

    pub(super) fn full_scroll(&mut self, by: isize) {
        let (lines, page) = self.full_page();
        if let Some(full) = self.full.as_mut() {
            full.scroll(by, &lines, page);
        }
    }

    /// Keys while the full view is open: it scrolls, answers, jumps or goes back; the board's
    /// own keys wait.
    pub(super) fn full_key(&mut self, key: K, now: Instant) -> Option<Effect> {
        let page = self.full_page().1 as isize;
        let act = match self.pending.take() {
            Some(prefix) => keys::find(keys::FULL, &[keys::K(KeyCode::Char(prefix), false), key]),
            None if keys::starts(keys::FULL, key) => {
                self.pending = key.char();
                return None;
            }
            None => keys::find(keys::FULL, &[key]),
        };
        match act? {
            Act::Leave => self.full = None,
            Act::Quit => return Some(Effect::Quit),
            Act::Down => self.full_scroll(-1),
            Act::Up => self.full_scroll(1),
            Act::HalfDown => self.full_scroll(-(page / 2).max(1)),
            Act::HalfUp => self.full_scroll((page / 2).max(1)),
            Act::PageDown => self.full_scroll(-page),
            Act::PageUp => self.full_scroll(page),
            Act::First => self.full_scroll(isize::MAX),
            Act::Last => self.full_scroll(isize::MIN),
            // The filter or a fold may have moved the board's selection off the viewed row.
            act @ (Act::Answer | Act::Jump) => {
                self.selected = self.full.as_ref().map(|full| Spot::Row(full.key.clone()));
                return if act == Act::Jump {
                    self.jump()
                } else {
                    self.start_answer(now)
                };
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    #[test]
    fn full_view_a_and_enter_act_on_the_viewed_row_not_the_boards_selection() {
        let mut app = viewing_pi_as_it_leaves_the_filter();
        let effect = press(&mut app, KeyCode::Char('a'));
        assert!(
            matches!(effect, Some(Effect::Read { ref key, .. }) if key == "mini/pi"),
            "{effect:?}"
        );
        assert_eq!(
            app.answer.as_ref().map(|a| a.row.key.as_str()),
            Some("mini/pi")
        );
        let mut app = viewing_pi_as_it_leaves_the_filter();
        let effect = press(&mut app, KeyCode::Enter);
        assert!(
            matches!(effect, Some(Effect::Jump(ref row)) if row.key == "mini/pi"),
            "{effect:?}"
        );
    }

    #[test]
    fn full_view_a_answers_the_viewed_row_when_its_group_is_folded() {
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        press(&mut app, KeyCode::Char(' '));
        app.folded.insert(Bucket::Status(Group::Working));
        let mut rows = rows();
        rows[0] = row("mini/seat-b", "working", "working", false);
        app.set_rows(rows, hosts());
        assert!(!matches!(&app.selected, Some(Spot::Row(key)) if key == "mini/seat-b"));
        let effect = press(&mut app, KeyCode::Char('a'));
        assert!(
            matches!(effect, Some(Effect::Read { ref key, .. }) if key == "mini/seat-b"),
            "{effect:?}"
        );
    }

    #[test]
    fn full_view_says_when_its_row_leaves_the_board() {
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        press(&mut app, KeyCode::Char(' '));
        app.set_rows(rows().split_off(1), hosts());
        assert!(app.full.is_none());
        assert_eq!(app.toast(), Some("seat-b left the board"));
        assert!(
            render(&app, 80, 30).contains("WORKING"),
            "the board draws again"
        );
    }
}
