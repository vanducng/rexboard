//! Where the selection rests and how it moves: the list's spots, motions, folds, the mouse and
//! how fast the board must redraw for what is on screen.

use super::*;

impl App {
    /// Headers and rows in the view's groups and order, filtered, with folded groups showing
    /// only their header.
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for (bucket, members) in arrange(&self.rows, &self.view, self.sort) {
            let members: Vec<usize> = members
                .into_iter()
                .filter(|&i| self.matches(&self.rows[i]))
                .collect();
            if members.is_empty() {
                continue;
            }
            let Some(bucket) = bucket else {
                items.extend(members.into_iter().map(Item::Row));
                continue;
            };
            let folded = self.folded.contains(&bucket);
            items.push(Item::Header {
                bucket,
                count: members.len(),
                folded,
            });
            if !folded {
                items.extend(members.into_iter().map(Item::Row));
            }
        }
        items
    }

    /// Where the selection can rest, in list order: rows and folded headers.
    pub fn spots(&self) -> Vec<Spot> {
        self.items()
            .iter()
            .filter_map(|item| self.spot(item))
            .collect()
    }

    /// The spot an item is, if the selection can rest on it: a row, or a folded header.
    pub(super) fn spot(&self, item: &Item) -> Option<Spot> {
        match item {
            Item::Row(i) => Some(Spot::Row(self.rows[*i].key.clone())),
            Item::Header {
                bucket,
                folded: true,
                ..
            } => Some(Spot::Group(bucket.clone())),
            Item::Header { .. } => None,
        }
    }

    pub fn current(&self) -> Option<&Row> {
        let Some(Spot::Row(key)) = &self.selected else {
            return None;
        };
        self.rows.iter().find(|row| &row.key == key)
    }

    /// Moves the selection, wrapping at both ends.
    pub(super) fn step(&mut self, by: isize) {
        let spots = self.spots();
        if spots.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|spot| spots.iter().position(|s| s == spot));
        let next = match at {
            None => 0,
            Some(at) => (at as isize + by).rem_euclid(spots.len() as isize) as usize,
        };
        self.selected = Some(spots[next].clone());
    }

    pub(super) fn first_row_of(&self, group: &Bucket) -> Option<Spot> {
        self.items().into_iter().find_map(|item| match item {
            Item::Row(i) if bucket(&self.view, &self.rows[i]).as_ref() == Some(group) => {
                Some(Spot::Row(self.rows[i].key.clone()))
            }
            _ => None,
        })
    }

    /// The spots drawn on screen now, in order.
    pub(super) fn on_screen_spots(&self) -> Vec<Spot> {
        let (_, offset, room) = self.drawn.get();
        self.items()
            .iter()
            .skip(offset)
            .take(room.max(1))
            .filter_map(|item| self.spot(item))
            .collect()
    }

    /// `H` `M` `L`: the top, middle or bottom of what the screen shows.
    pub(super) fn on_screen(&mut self, act: Act) {
        let shown = self.on_screen_spots();
        let at = match act {
            Act::Top => 0,
            Act::Middle => shown.len().saturating_sub(1) / 2,
            _ => shown.len().saturating_sub(1),
        };
        if let Some(spot) = shown.get(at) {
            self.selected = Some(spot.clone());
        }
    }

    /// `ctrl-d`/`ctrl-u` (a half) and `ctrl-f`/`ctrl-b` (a whole): a page of the screen's
    /// rows down (`dir` 1) or up, stopping at the ends.
    pub(super) fn page(&mut self, dir: isize, part: usize) {
        let (_, _, room) = self.drawn.get();
        let by = (room / part).max(1) as isize;
        let spots = self.spots();
        let Some(last) = spots.len().checked_sub(1) else {
            return;
        };
        let at = self
            .selected
            .as_ref()
            .and_then(|spot| spots.iter().position(|s| s == spot))
            .unwrap_or(0) as isize;
        let next = (at + dir * by).clamp(0, last as isize) as usize;
        self.selected = Some(spots[next].clone());
    }

    /// `]` and `[`: the first spot of the next group, or of this one and then the one before.
    pub(super) fn group(&mut self, back: bool) {
        let items = self.items();
        let spot_at = |i: usize| items[i..].iter().find_map(|item| self.spot(item));
        let heads: Vec<usize> = (0..items.len())
            .filter(|&i| matches!(items[i], Item::Header { .. }))
            .collect();
        let here = items.iter().position(|item| {
            let spot = match item {
                Item::Row(r) => Spot::Row(self.rows[*r].key.clone()),
                Item::Header { bucket, .. } => Spot::Group(bucket.clone()),
            };
            Some(&spot) == self.selected.as_ref()
        });
        let Some(here) = here else {
            self.selected = spot_at(0);
            return;
        };
        let head = if back {
            let own = heads.iter().rev().find(|&&h| h <= here).copied();
            match own {
                Some(h) if spot_at(h) != self.selected => Some(h),
                Some(h) => heads.iter().rev().find(|&&o| o < h).copied(),
                None => None,
            }
        } else {
            heads.iter().find(|&&h| h > here).copied()
        };
        if let Some(spot) = head.and_then(spot_at) {
            self.selected = Some(spot);
        }
    }

    /// `1`-`9`: the nth NEEDS YOU row, opening its group if it is folded.
    pub(super) fn nth(&mut self, n: usize) {
        for row in self.rows.iter().filter(|row| row.group == Group::NeedsYou) {
            if let Some(group) = bucket(&self.view, row) {
                self.folded.remove(&group);
            }
        }
        let pick = self
            .items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) if self.rows[i].group == Group::NeedsYou => {
                    Some(Spot::Row(self.rows[i].key.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        match pick.get(n) {
            Some(spot) => self.selected = Some(spot.clone()),
            None if pick.is_empty() => self.say("nobody needs you"),
            None => self.say(format!("NEEDS YOU has {} rows shown", pick.len())),
        }
    }

    /// `n` (or `tab`, or `N` the other way): a done row read for READ is seen; then on to the
    /// next NEEDS YOU row, wrapping.
    pub(super) fn next_waiting(&mut self, now: Instant, back: bool) -> Option<Effect> {
        let seen = self.current().and_then(|row| {
            let read = self
                .shown
                .as_ref()
                .is_some_and(|(key, since)| *key == row.key && now.duration_since(*since) >= READ);
            (read && row.pane.state == "done" && !row.seen).then(|| Effect::Seen {
                key: row.key.clone(),
                label: path(&labels(row, true), usize::MAX),
                state: row.pane.state.clone(),
            })
        });
        for row in self.rows.iter().filter(|row| row.group == Group::NeedsYou) {
            if let Some(group) = bucket(&self.view, row) {
                self.folded.remove(&group);
            }
        }
        let waiting: Vec<Spot> = self
            .items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) if self.rows[i].group == Group::NeedsYou => {
                    Some(Spot::Row(self.rows[i].key.clone()))
                }
                _ => None,
            })
            .collect();
        let at = waiting
            .iter()
            .position(|spot| Some(spot) == self.selected.as_ref());
        let n = waiting.len().max(1);
        let next = match at {
            Some(at) if back => (at + n - 1) % n,
            Some(at) => (at + 1) % n,
            None if back => n - 1,
            None => 0,
        };
        match waiting.get(next) {
            Some(spot) => self.selected = Some(spot.clone()),
            None => self.say("nobody needs you"),
        }
        seen
    }

    /// `za` toggles the selection's group, `zo` opens a folded one, `zc` folds a row's.
    pub(super) fn fold(&mut self, act: Act) {
        match self.selected.clone() {
            Some(Spot::Group(group)) if act != Act::FoldClose => {
                self.folded.remove(&group);
                self.selected = self.first_row_of(&group);
            }
            Some(Spot::Row(_)) if act != Act::FoldOpen => {
                if let Some(group) = self.current().and_then(|row| bucket(&self.view, row)) {
                    self.folded.insert(group.clone());
                    self.selected = Some(Spot::Group(group));
                }
            }
            _ => {}
        }
    }

    /// `zM`: every group folded; the selection rests on its group's header.
    pub(super) fn fold_all(&mut self) {
        let groups: Vec<Bucket> = self
            .items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Header { bucket, .. } => Some(bucket),
                Item::Row(_) => None,
            })
            .collect();
        let mine = self.current().and_then(|row| bucket(&self.view, row));
        self.folded.extend(groups);
        if let Some(group) = mine {
            self.selected = Some(Spot::Group(group));
        }
    }

    pub(super) fn unfold_all(&mut self) {
        self.folded.clear();
        if let Some(Spot::Group(group)) = self.selected.clone() {
            self.selected = self.first_row_of(&group);
        }
    }

    pub(super) fn keep_selection_visible(&mut self) {
        let spots = self.spots();
        if !self
            .selected
            .as_ref()
            .is_some_and(|spot| spots.contains(spot))
        {
            self.selected = spots.first().cloned();
        }
    }

    /// Whether clicks sort: in the table view, which turns mouse capture on, and not in pick.
    pub fn clicks(&self) -> bool {
        self.view.group_by.is_none() && !self.pick
    }

    /// Whether a prompt (`/`, `:` or `f`) has the keyboard, so the mouse leaves the list alone.
    pub(super) fn prompting(&self) -> bool {
        self.filtering || self.commanding || self.pending == Some('f') || self.answer.is_some()
    }

    /// A click at `x`, `y` in the table view: on a column's header cell it sorts as that
    /// column's letter does, on its arrow it flips the sort, and on a row it selects the row.
    pub fn click(&mut self, x: u16, y: u16) {
        use crate::tui::view::{HEADED, HEADER_ROW};
        let items = self.items();
        let list = usize::from(self.width);
        if !self.clicks() || self.prompting() || items.is_empty() || self.full.is_some() {
            return;
        }
        if self.height >= HEADED && y == HEADER_ROW {
            let columns = Columns::new(self.width, list, &self.field_columns());
            if let Some(letter) = columns.at(usize::from(x), self.sort).and_then(|c| c.letter) {
                self.sort_by(letter);
            }
            return;
        }
        let (first, offset, room) = self.drawn.get();
        if let Some(below) = y.checked_sub(first).map(usize::from)
            && below < room
            && let Some(&Item::Row(i)) = items.get(offset + below)
        {
            self.selected = Some(Spot::Row(self.rows[i].key.clone()));
        }
    }

    /// The wheel moves the selection, as `k` and `j` do, but stops at the ends; with the mouse
    /// captured the terminal no longer turns it into arrow keys.
    pub fn scroll(&mut self, up: bool) {
        if self.full.is_some() {
            return self.full_scroll(if up { 3 } else { -3 });
        }
        if self.prompting() {
            return;
        }
        let spots = self.spots();
        let Some(last) = spots.len().checked_sub(1) else {
            return;
        };
        let at = self
            .selected
            .as_ref()
            .and_then(|spot| spots.iter().position(|s| s == spot));
        let next = match at {
            None => 0,
            Some(at) if up => at.saturating_sub(1),
            Some(at) => (at + 1).min(last),
        };
        self.selected = Some(spots[next].clone());
    }

    /// How often the board must redraw for the glyphs on screen to move: the full view's row, or
    /// the list's visible rows and the card's; None when nothing on screen animates.
    pub fn frame_every(&self) -> Option<Duration> {
        if let Some(full) = &self.full {
            let row = self.rows.iter().find(|row| row.key == full.key);
            return crate::tui::theme::redraw_every(
                row.map(crate::tui::theme::look),
                crate::tui::theme::motion(),
            );
        }
        let (_, offset, room) = self.drawn.get();
        let items = self.items();
        let listed = items
            .iter()
            .skip(offset)
            .take(room)
            .filter_map(|item| match item {
                Item::Row(i) => Some(&self.rows[*i]),
                Item::Header { .. } => None,
            });
        let carded = self.current().filter(|_| self.card_shown());
        crate::tui::theme::redraw_every(
            listed.chain(carded).map(crate::tui::theme::look),
            crate::tui::theme::motion(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    #[test]
    fn selection_skips_group_headers_and_wraps() {
        let mut app = app();
        assert_eq!(selected(&app), "mini/seat-b");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(selected(&app), "mini/codex", "past the WORKING header");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(selected(&app), "mini/seat-b", "wraps to the top");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(selected(&app), "local/seat-a", "wraps to the bottom");
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(
            selected(&app),
            "local/claude",
            "2 is the second NEEDS YOU row"
        );
    }

    #[test]
    fn motions_move_by_screen_page_group_and_needing_you() {
        let mut app = App::default();
        let mut rows: Vec<Row> = (0..3)
            .map(|i| row(&format!("mini/b{i}"), "blocked", "needs_you", false))
            .collect();
        rows.extend((0..10).map(|i| row(&format!("local/w{i}"), "working", "working", false)));
        rows.push(row("local/idle", "idle", "quiet", true));
        app.set_rows(rows, hosts());
        // In list order: NEEDS YOU's 3 rows, WORKING's 10, QUIET's 1.
        let spots = app.spots();
        let at = |app: &App| spots.iter().position(|s| Some(s) == app.selected.as_ref());
        // 8 items from the top: NEEDS YOU, its 3 rows, WORKING and its first 3 rows.
        app.drawn.set((3, 0, 8));
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(at(&app), Some(5), "the last row on screen");
        press(&mut app, KeyCode::Char('H'));
        assert_eq!(at(&app), Some(0));
        press(&mut app, KeyCode::Char('M'));
        assert_eq!(at(&app), Some(2), "the middle of the 6 rows on screen");
        assert_eq!(
            app.toast(),
            Some("middle of the screen · M used to sort; message is om now")
        );
        key(&mut app, ctrl('d'));
        assert_eq!(at(&app), Some(6), "half of 8 down");
        key(&mut app, ctrl('f'));
        assert_eq!(at(&app), Some(13), "a page, stopping at the end");
        key(&mut app, ctrl('b'));
        key(&mut app, ctrl('u'));
        assert_eq!(at(&app), Some(1));
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(at(&app), Some(3), "the next group's first row");
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(at(&app), Some(13));
        press(&mut app, KeyCode::Char('['));
        assert_eq!(at(&app), Some(3), "the previous group's first row");
        key(&mut app, KeyEvent::from(KeyCode::Down));
        press(&mut app, KeyCode::Char('['));
        assert_eq!(at(&app), Some(3), "first back to its own group's first row");
        press(&mut app, KeyCode::Char('['));
        assert_eq!(at(&app), Some(0));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(at(&app), Some(1));
        press(&mut app, KeyCode::Char('N'));
        assert_eq!(at(&app), Some(0));
        press(&mut app, KeyCode::Char('N'));
        assert_eq!(at(&app), Some(2), "N wraps back");
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(at(&app), Some(13));
        typed(&mut app, "gg");
        assert_eq!(at(&app), Some(0));
        typed(&mut app, "gt");
        assert_eq!(app.view.name, "table");
        typed(&mut app, "gT");
        assert_eq!(app.view.name, "status");
        press(&mut app, KeyCode::Char('V'));
        assert_ne!(app.view.name, "status", "V goes the other way round");
        assert!(matches!(key(&mut app, ctrl('o')), Some(Effect::Back)));
    }

    #[test]
    fn za_folds_then_unfolds_from_the_folded_header_and_zo_zc_go_one_way() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        typed(&mut app, "za");
        assert_eq!(
            app.selected,
            Some(Spot::Group(Bucket::Status(Group::Working))),
            "rests on the folded header"
        );
        assert!(app.items().contains(&Item::Header {
            bucket: Bucket::Status(Group::Working),
            count: 1,
            folded: true
        }));
        typed(&mut app, "za");
        assert!(app.folded.is_empty(), "za on a folded header opens it");
        assert_eq!(selected(&app), "mini/codex");
        typed(&mut app, "zo");
        assert!(app.folded.is_empty(), "zo on a row does nothing");
        typed(&mut app, "zc");
        typed(&mut app, "zc");
        assert_eq!(app.folded.len(), 1, "zc on a folded header does nothing");
        typed(&mut app, "zo");
        assert!(app.folded.is_empty());
    }

    #[test]
    fn zm_folds_every_group_and_zr_opens_them_all() {
        let mut app = app();
        typed(&mut app, "zM");
        assert_eq!(app.folded.len(), 3);
        assert!(
            app.spots()
                .iter()
                .all(|spot| matches!(spot, Spot::Group(_)))
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.folded.len(), 3, "esc no longer opens folds");
        typed(&mut app, "zR");
        assert!(app.folded.is_empty());
        assert!(app.current().is_some());
    }

    #[test]
    fn the_board_redraws_fast_only_while_a_working_row_is_on_screen() {
        let spin = Some(Duration::from_millis(160));
        let mut app = app();
        app.drawn.set((3, 0, 20));
        assert_eq!(app.frame_every(), spin, "codex is in the list");
        app.drawn.set((3, 0, 3));
        assert_eq!(
            app.frame_every(),
            None,
            "only NEEDS YOU fits, and the card is seat-b's"
        );
        app.selected = Some(Spot::Row("mini/codex".into()));
        assert_eq!(app.frame_every(), spin, "the card's bar is codex's");
        press(&mut app, KeyCode::Char(' '));
        assert!(app.full.is_some());
        assert_eq!(app.frame_every(), spin, "the full view's bar");
        press(&mut app, KeyCode::Esc);
        app.selected = Some(Spot::Row("local/seat-a".into()));
        press(&mut app, KeyCode::Char(' '));
        assert!(app.full.is_some());
        assert_eq!(app.frame_every(), None, "an idle row's full view");
    }
}
