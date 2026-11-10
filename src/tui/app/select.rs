//! Where the selection rests and how it moves: the list's spots, motions, folds, the mouse and
//! how fast the board must redraw for what is on screen.

use super::*;

impl App {
    /// The list as last drawn: with blank rows between the groups when they all fit.
    pub fn items(&self) -> Vec<Item> {
        self.listed(self.spaced.get())
    }

    /// Headers and rows in the view's groups and order, filtered, with folded groups showing
    /// only their header, and with `gaps` a blank row above each header but the first.
    pub fn listed(&self, gaps: bool) -> Vec<Item> {
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
            if gaps && !items.is_empty() {
                items.push(Item::Gap);
            }
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
            Item::Header { .. } | Item::Gap => None,
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
                Item::Gap => return false,
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

    /// The NEEDS YOU rows in list order, after opening every group that holds one.
    fn needs_you_spots(&mut self) -> Vec<Spot> {
        for row in self.rows.iter().filter(|row| row.group == Group::NeedsYou) {
            if let Some(group) = bucket(&self.view, row) {
                self.folded.remove(&group);
            }
        }
        self.items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) if self.rows[i].group == Group::NeedsYou => {
                    Some(Spot::Row(self.rows[i].key.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The marked rows the view shows, in list order.
    fn marked_keys(&self) -> Vec<String> {
        self.items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) if self.rows[i].marked => Some(self.rows[i].key.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether a marked row passes the filters, folded or not, since `'` opens its group: what
    /// the footer's hint follows.
    pub fn marked_shown(&self) -> bool {
        self.rows.iter().any(|row| row.marked && self.matches(row))
    }

    /// `*`: the selected pane becomes a favorite, or stops being one. Serve keeps the list, so
    /// the row changes when serve says so.
    pub(super) fn favorite(&mut self) -> Option<Effect> {
        let row = self.current()?;
        let Some(id) = crate::favorites::Id::of(row) else {
            self.say("this pane has no label of its own · label it in Rex to favorite it");
            return None;
        };
        let on = !row
            .marked_by
            .iter()
            .any(|s| s == crate::favorites::FAVORITE);
        Some(Effect::Favorite {
            label: id.pane.clone(),
            id: Box::new(id),
            on,
        })
    }

    /// `'`: the first marked row in list order, after opening the groups that hold one as `n`
    /// does, and a jump to its pane; pressed again within 2 s, the next, wrapping.
    pub(super) fn go_marked(&mut self, now: Instant) -> Option<Effect> {
        if self.busy() {
            return None;
        }
        for row in self.rows.iter().filter(|row| row.marked) {
            if let Some(group) = bucket(&self.view, row) {
                self.folded.remove(&group);
            }
        }
        let marked = self.marked_keys();
        if marked.is_empty() {
            let rules = self.rows.iter().any(|row| row.marked);
            self.say(if rules {
                "no marked row in this view"
            } else {
                "no marked row · * marks one, or see [[mark]] in config.toml"
            });
            return None;
        }
        let next = self
            .marked_at
            .take()
            .filter(|(_, at)| now.duration_since(*at) <= MARKED_WINDOW)
            .and_then(|(key, _)| marked.iter().position(|k| *k == key))
            .map_or(0, |at| (at + 1) % marked.len());
        let key = marked[next].clone();
        self.marked_at = Some((key.clone(), now));
        self.selected = Some(Spot::Row(key));
        self.jump()
    }

    /// The rows drawn on screen now, in order, as indexes into `rows`: row `n` is `[n - 1]`.
    fn numbered(&self) -> Vec<usize> {
        let (_, offset, room) = self.drawn.get();
        let items = self.items();
        let rows = items
            .iter()
            .skip(offset)
            .take(room)
            .filter_map(|item| match item {
                Item::Row(i) => Some(*i),
                _ => None,
            });
        rows.take(NUMBERED).collect()
    }

    /// `1`-`9`: goes to that row of the screen at once (selects it, or jumps to it with `jump`),
    /// unless a two-digit number starts with it, which waits for the second digit.
    pub(super) fn number_key(&mut self, digit: usize, now: Instant, jump: bool) -> Option<Effect> {
        if digit * 10 <= self.numbered().len() {
            self.number = Some((digit, now, jump));
            return None;
        }
        self.go_number(digit, jump)
    }

    /// Selects row `n` of the screen, and with `jump` goes to its pane; or says why not.
    pub(super) fn go_number(&mut self, n: usize, jump: bool) -> Option<Effect> {
        let rows = self.numbered();
        match rows.get(n.wrapping_sub(1)) {
            Some(&i) => {
                self.selected = Some(Spot::Row(self.rows[i].key.clone()));
                return if jump { self.jump() } else { None };
            }
            None if rows.is_empty() => self.say("no rows on screen"),
            None => self.say(format!("no row {n} · rows 1-{} on screen", rows.len())),
        }
        None
    }

    /// A first digit that waited long enough for a second goes to its own row. Whether the board
    /// changed, and the jump when that is what the digit does, or a jump parked behind another
    /// key's effect.
    pub fn number_due(&mut self, now: Instant) -> (bool, Option<Effect>) {
        if let Some(effect) = self.deferred.take() {
            return (true, Some(effect));
        }
        let Some((first, at, jump)) = self.number else {
            return (false, None);
        };
        if now.duration_since(at) < NUMBER_WAIT {
            return (false, None);
        }
        self.number = None;
        (true, self.go_number(first, jump))
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
        let waiting = self.needs_you_spots();
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
                Item::Row(_) | Item::Gap => None,
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
            let columns = self.columns(self.width, list);
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

    /// The wheel at row `y` moves the selection, as `k` and `j` do, but stops at the ends; over
    /// the card it scrolls the card's text. With the mouse captured the terminal no longer turns
    /// it into arrow keys.
    pub fn scroll(&mut self, y: u16, up: bool) {
        if self.full.is_some() {
            return self.full_scroll(if up { 3 } else { -3 });
        }
        if self.prompting() {
            return;
        }
        let card = self.card_drawn.get();
        if (card.top..card.bottom).contains(&y) {
            return self.card_scroll(if up { -1 } else { 1 });
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
                Item::Header { .. } | Item::Gap => None,
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
    }

    /// `count` working rows `local/r0..`, drawn once at `height` rows tall.
    fn crowd(count: usize, height: u16) -> App {
        let mut app = App::default();
        let rows = (0..count).map(|i| row(&format!("local/r{i}"), "working", "working", true));
        app.set_rows(rows.collect(), hosts());
        render(&app, 100, height);
        app
    }

    fn key_at(app: &mut App, c: char, at: Instant) -> Option<Effect> {
        app.handle_at(KeyEvent::from(KeyCode::Char(c)), at)
    }

    #[test]
    fn the_number_column_numbers_rows_by_place_and_leaves_headers_blank() {
        let text = render(&app(), 80, 30);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[2].starts_with("  #  "), "{}", lines[2]);
        let heads: Vec<&&str> = lines.iter().filter(|l| l.contains(" (")).collect();
        assert!(heads.iter().all(|l| !l.starts_with(' ')), "{heads:?}");
        for (n, who) in [(1, "seat-b"), (2, "claude"), (3, "codex"), (4, "seat-a")] {
            let row = lines
                .iter()
                .find(|l| l.contains(&format!("{who} msg")))
                .unwrap();
            assert!(row.starts_with(&format!("  {n} ")), "{n}: {row}");
        }
    }

    #[test]
    fn a_digit_no_two_digit_number_starts_with_selects_at_once() {
        for count in [4, 9, 19] {
            let mut app = crowd(count, 30);
            let now = Instant::now();
            // Only 1 has a two-digit number to wait for (10-19), and only when the screen has 10.
            key_at(&mut app, '2', now);
            assert_eq!(selected(&app), "local/r1", "{count} rows: 2 at once");
            assert!(app.number.is_none());
        }
        let mut app = crowd(9, 30);
        key_at(&mut app, '1', Instant::now());
        assert_eq!(selected(&app), "local/r0", "9 rows: no 10, so 1 is at once");
    }

    #[test]
    fn a_digit_that_starts_a_two_digit_number_waits_for_a_second_digit_or_400_ms() {
        let mut app = crowd(12, 30);
        let t = Instant::now();
        key_at(&mut app, '1', t);
        assert_eq!(app.number.map(|n| n.0), Some(1), "waiting");
        assert_eq!(
            selected(&app),
            "local/r0",
            "nothing selected yet; the first row was"
        );
        let text = render(&app, 100, 30);
        assert!(text.lines().last().unwrap().starts_with("1… "), "{text}");
        assert!(!app.number_due(t + Duration::from_millis(399)).0);
        assert!(app.number_due(t + Duration::from_millis(400)).0);
        assert_eq!((selected(&app), app.number), ("local/r0".into(), None));
        // A second digit selects that row, with no wait.
        key_at(&mut app, '1', t);
        key_at(&mut app, '2', t + Duration::from_millis(50));
        assert_eq!((selected(&app), app.number), ("local/r11".into(), None));
        // 2 starts nothing with 12 rows, so it never waits.
        key_at(&mut app, '2', t);
        assert_eq!(selected(&app), "local/r1");
    }

    #[test]
    fn enter_ends_the_wait_esc_cancels_and_another_key_ends_it_then_acts() {
        let mut app = crowd(12, 30);
        let t = Instant::now();
        key_at(&mut app, '3', t);
        assert_eq!(selected(&app), "local/r2", "3 starts nothing with 12 rows");
        key_at(&mut app, '1', t);
        let jumped = app.handle_at(KeyEvent::from(KeyCode::Enter), t);
        assert_eq!(
            (jumped, selected(&app)),
            (None, "local/r0".into()),
            "enter selects 1, no jump"
        );
        key_at(&mut app, '6', t);
        key_at(&mut app, '1', t);
        app.handle_at(KeyEvent::from(KeyCode::Esc), t);
        assert_eq!(
            (selected(&app), app.number),
            ("local/r5".into(), None),
            "esc cancels"
        );
        key_at(&mut app, '1', t);
        key_at(&mut app, '1', t);
        assert_eq!(selected(&app), "local/r10");
        key_at(&mut app, '1', t);
        app.handle_at(KeyEvent::from(KeyCode::Enter), t);
        assert_eq!(selected(&app), "local/r0");
        key_at(&mut app, '5', t);
        key_at(&mut app, '1', t);
        key_at(&mut app, 'j', t);
        assert_eq!(
            selected(&app),
            "local/r1",
            "1 selected r0, then j moved down"
        );
        key_at(&mut app, '1', t);
        key_at(&mut app, '9', t);
        assert_eq!(app.toast(), Some("no row 19 · rows 1-12 on screen"));
        assert_eq!(selected(&app), "local/r1", "unchanged");
    }

    /// The block named on the list line numbered `n`, as the row's key.
    fn numbered_as_drawn(text: &str, n: usize) -> String {
        let line = text
            .lines()
            .find(|l| l.starts_with(&format!("{n:>3} ")))
            .unwrap();
        let words: Vec<&str> = line.split_whitespace().collect();
        format!("local/{}", words[words.len() - 2])
    }

    #[test]
    fn numbers_follow_the_screen_when_it_scrolls_and_the_filter_when_it_narrows() {
        let mut app = crowd(30, 12);
        let t = Instant::now();
        press(&mut app, KeyCode::Char('G'));
        let text = render(&app, 100, 12);
        let first = numbered_as_drawn(&text, 1);
        assert_ne!(first, "local/r0", "the screen shows the tail:\n{text}");
        assert!(text.contains("more") || selected(&app) == "local/r29");
        key_at(&mut app, '1', t);
        assert_eq!(selected(&app), first, "1 is the top of the screen");
        let mut app = crowd(12, 30);
        typed(&mut app, "/r1");
        press(&mut app, KeyCode::Enter);
        let text = render(&app, 100, 30);
        assert!(!text.lines().any(|l| l.starts_with(" 4 ")), "{text}");
        let third = numbered_as_drawn(&text, 3);
        key_at(&mut app, '3', t);
        assert_eq!(
            selected(&app),
            third,
            "3 is the third row the filter leaves"
        );
        key_at(&mut app, '4', t);
        assert_eq!(app.toast(), Some("no row 4 · rows 1-3 on screen"));
    }

    #[test]
    fn numbers_work_in_every_view_and_in_pick() {
        for view in ["table", "harness", "host"] {
            let mut app = harnesses(view);
            let text = render(&app, 120, 30);
            assert!(
                text.lines().any(|l| l.starts_with("  1 ")),
                "{view}\n{text}"
            );
            key_at(&mut app, '2', Instant::now());
            assert!(app.current().is_some(), "{view}");
        }
        let mut app = crowd(5, 30);
        app.pick = true;
        key_at(&mut app, '3', Instant::now());
        assert_eq!(selected(&app), "local/r2");
    }

    /// `crowd`, with tui.json's `"digits"` as `value`.
    fn digits(count: usize, value: Option<&str>) -> App {
        let mut app = crowd(count, 30);
        app.tuning.digits = value.map(|v| v.into());
        app
    }

    /// The key of the row a jump goes to, and None for anything else.
    fn jumped(effect: Option<Effect>) -> Option<String> {
        match effect {
            Some(Effect::Jump(row)) => Some(row.key),
            _ => None,
        }
    }

    fn g(app: &mut App, c: char, at: Instant) -> Option<Effect> {
        key_at(app, 'g', at);
        key_at(app, c, at)
    }

    #[test]
    fn digits_select_by_default_and_jump_when_asked_and_g_does_the_other() {
        let t = Instant::now();
        for value in [None, Some("select"), Some("nope")] {
            let mut app = digits(5, value);
            assert_eq!(jumped(key_at(&mut app, '3', t)), None, "{value:?}");
            assert_eq!(selected(&app), "local/r2");
            assert_eq!(jumped(g(&mut app, '4', t)).as_deref(), Some("local/r3"));
        }
        let mut app = digits(5, Some("jump"));
        assert_eq!(
            jumped(key_at(&mut app, '3', t)).as_deref(),
            Some("local/r2")
        );
        app.going = None;
        assert_eq!(jumped(g(&mut app, '4', t)), None);
        assert_eq!(
            selected(&app),
            "local/r3",
            "g and a number selects under jump"
        );
    }

    #[test]
    fn the_two_digit_wait_holds_in_both_modes_for_both_actions() {
        let t = Instant::now();
        let late = t + Duration::from_millis(400);
        for (value, plain_jumps) in [(None, false), (Some("jump"), true)] {
            let mut app = digits(12, value);
            // The plain digits: a wait, then the row at 400 ms, or a second digit, or enter.
            assert_eq!(jumped(key_at(&mut app, '1', t)), None);
            assert!(app.number.is_some(), "{value:?}: waiting");
            let footer = render(&app, 100, 30);
            let word = if plain_jumps {
                "enter jumps"
            } else {
                "enter selects"
            };
            assert!(footer.lines().last().unwrap().contains(word), "{footer}");
            let (due, effect) = app.number_due(late);
            assert!(due);
            assert_eq!(
                jumped(effect).is_some(),
                plain_jumps,
                "{value:?}: after 400 ms"
            );
            assert_eq!(selected(&app), "local/r0");
            app.going = None;
            key_at(&mut app, '1', t);
            let second = key_at(&mut app, '2', t);
            assert_eq!(jumped(second).as_deref().is_some(), plain_jumps);
            assert_eq!(selected(&app), "local/r11");
            app.going = None;
            key_at(&mut app, '1', t);
            let enter = app.handle_at(KeyEvent::from(KeyCode::Enter), t);
            assert_eq!(jumped(enter).is_some(), plain_jumps, "{value:?}: enter");
            assert_eq!(selected(&app), "local/r0");
            app.going = None;
            key_at(&mut app, '1', t);
            let esc = app.handle_at(KeyEvent::from(KeyCode::Esc), t);
            assert_eq!((esc.is_none(), app.number), (true, None), "esc cancels");
            // After `g` the same wait, doing the other action.
            g(&mut app, '1', t);
            assert!(app.number.is_some(), "{value:?}: g1 waits");
            let (_, effect) = app.number_due(late);
            assert_eq!(
                jumped(effect).is_some(),
                !plain_jumps,
                "{value:?}: g after 400 ms"
            );
            app.going = None;
            g(&mut app, '1', t);
            let second = key_at(&mut app, '2', t);
            assert_eq!(jumped(second).as_deref().is_some(), !plain_jumps);
            assert_eq!(selected(&app), "local/r11");
            app.going = None;
        }
    }

    #[test]
    fn a_key_that_ends_the_wait_does_its_own_job_after_a_jump() {
        let t = Instant::now();
        for (value, plain_jumps) in [(None, false), (Some("jump"), true)] {
            let mut app = digits(12, value);
            key_at(&mut app, '1', t);
            let effect = key_at(&mut app, 'j', t);
            assert_eq!(jumped(effect).is_some(), plain_jumps, "{value:?}");
            assert_eq!(
                selected(&app),
                "local/r1",
                "{value:?}: 1 went to r0, then j moved"
            );
            assert!(app.number.is_none());
            app.going = None;
            g(&mut app, '1', t);
            let effect = key_at(&mut app, 'j', t);
            assert_eq!(jumped(effect).is_some(), !plain_jumps, "{value:?} g");
            assert_eq!(selected(&app), "local/r1", "{value:?} g: moved after");
        }
    }

    #[test]
    fn a_key_with_an_effect_of_its_own_keeps_it_and_the_jump_waits_for_the_loop() {
        let t = Instant::now();
        let mut app = digits(12, Some("jump"));
        key_at(&mut app, '1', t);
        let own = key_at(&mut app, ' ', t);
        assert!(
            app.full.is_some() && own.is_some(),
            "the full view gets its read: {own:?}"
        );
        assert!(!matches!(own, Some(Effect::Jump(_))));
        let (changed, jump) = app.number_due(t);
        assert_eq!((changed, jumped(jump).as_deref()), (true, Some("local/r0")));
        assert_eq!(app.number_due(t), (false, None), "handed out once");
        let mut app = digits(12, Some("jump"));
        key_at(&mut app, '1', t);
        let own = key_at(&mut app, 'c', t);
        assert!(matches!(own, Some(Effect::SavePrefs(_))), "{own:?}");
        assert_eq!(jumped(app.number_due(t).1).as_deref(), Some("local/r0"));
        let mut app = digits(12, Some("jump"));
        key_at(&mut app, '1', t);
        assert_eq!(key_at(&mut app, 'q', t), Some(Effect::Quit), "quit wins");
        assert_eq!(app.number_due(t), (false, None));
    }

    #[test]
    fn a_number_with_no_row_says_so_in_both_modes_and_for_g() {
        let t = Instant::now();
        for value in [None, Some("jump")] {
            let mut app = digits(5, value);
            let before = selected(&app);
            assert_eq!(jumped(key_at(&mut app, '9', t)), None);
            assert_eq!(
                app.toast(),
                Some("no row 9 · rows 1-5 on screen"),
                "{value:?}"
            );
            app.toast = None;
            assert_eq!(jumped(g(&mut app, '8', t)), None);
            assert_eq!(
                app.toast(),
                Some("no row 8 · rows 1-5 on screen"),
                "{value:?} g"
            );
            assert_eq!(selected(&app), before, "the selection stays");
        }
    }

    #[test]
    fn pick_follows_the_setting_and_a_digit_picks_under_jump() {
        let t = Instant::now();
        let mut app = digits(5, Some("jump"));
        app.pick = true;
        assert_eq!(
            jumped(key_at(&mut app, '3', t)).as_deref(),
            Some("local/r2")
        );
        let mut app = digits(5, None);
        app.pick = true;
        assert_eq!(jumped(key_at(&mut app, '3', t)), None);
        assert_eq!(jumped(g(&mut app, '2', t)).as_deref(), Some("local/r1"));
    }

    /// `fuller`'s rows with `mini/seat-b` (the first) and `mini/codex` (the third) marked.
    fn marked() -> App {
        crate::tui::app::testkit::marked_board()
    }

    #[test]
    fn the_quote_goes_to_the_first_marked_row_and_again_within_2_s_to_the_next_wrapping() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut app = marked();
        app.selected = Some(Spot::Row("local/seat-a".into()));
        assert_eq!(
            jumped(key_at(&mut app, '\'', t)).as_deref(),
            Some("mini/seat-b")
        );
        assert_eq!(
            selected(&app),
            "mini/seat-b",
            "the first in list order, not after the selection"
        );
        app.going = None;
        assert_eq!(
            jumped(key_at(&mut app, '\'', ms(1500))).as_deref(),
            Some("mini/codex")
        );
        app.going = None;
        assert_eq!(
            jumped(key_at(&mut app, '\'', ms(3000))).as_deref(),
            Some("mini/seat-b"),
            "1.5 s after the last: the window moves with each press, and it wraps"
        );
        app.going = None;
        assert_eq!(
            jumped(key_at(&mut app, '\'', ms(5001))).as_deref(),
            Some("mini/seat-b"),
            "over 2 s after the last: back to the first"
        );
        app.going = None;
        assert_eq!(
            jumped(key_at(&mut app, '\'', ms(7000))).as_deref(),
            Some("mini/codex"),
            "exactly 2 s still counts"
        );
    }

    #[test]
    fn the_quote_walks_what_the_filter_and_a_folded_group_leave() {
        let t = Instant::now();
        let mut app = marked();
        app.folded.insert(Bucket::Status(Group::Working));
        assert!(
            !app.spots().contains(&Spot::Row("mini/codex".into())),
            "folded away"
        );
        key_at(&mut app, '\'', t);
        app.going = None;
        let next = key_at(&mut app, '\'', t + Duration::from_millis(100));
        assert_eq!(
            jumped(next).as_deref(),
            Some("mini/codex"),
            "the group opened, as n opens it"
        );
        assert!(app.spots().contains(&Spot::Row("mini/codex".into())));
        app.going = None;
        for c in "/codex".chars() {
            key_at(&mut app, c, t);
        }
        app.handle_at(KeyEvent::from(KeyCode::Enter), t);
        let later = t + Duration::from_secs(10);
        assert_eq!(
            jumped(key_at(&mut app, '\'', later)).as_deref(),
            Some("mini/codex"),
            "seat-b is filtered out"
        );
        app.going = None;
        let again = key_at(&mut app, '\'', later + Duration::from_millis(50));
        assert_eq!(
            jumped(again).as_deref(),
            Some("mini/codex"),
            "the only marked row wraps to itself"
        );
    }

    #[test]
    fn the_star_favorites_the_selected_pane_by_its_labels_or_unfavorites_it() {
        let mut app = marked();
        let effect = key_at(&mut app, '*', Instant::now());
        let Some(Effect::Favorite { id, on, label }) = effect else {
            panic!("want a favorite effect, got {effect:?}");
        };
        assert_eq!(
            (
                id.host.as_str(),
                id.session.as_str(),
                id.tab.as_str(),
                id.pane.as_str()
            ),
            ("mini", "api", "main", "seat-b"),
            "the labels, with no block id"
        );
        assert!(
            (on, label.as_str()) == (true, "seat-b"),
            "marked by config only: * adds"
        );
        let mut rows = app.rows.clone();
        rows[0].marked_by.push("favorite".into());
        app.set_rows(rows, app.hosts.clone());
        let effect = key_at(&mut app, '*', Instant::now());
        assert!(
            matches!(effect, Some(Effect::Favorite { on: false, .. })),
            "{effect:?}"
        );
    }

    #[test]
    fn a_pane_with_no_label_of_its_own_is_not_favorited_and_the_footer_says_what_to_do() {
        let mut app = marked();
        let mut rows = app.rows.clone();
        rows[0].pane.pane.clear();
        app.set_rows(rows, app.hosts.clone());
        assert_eq!(key_at(&mut app, '*', Instant::now()), None);
        assert_eq!(
            app.toast(),
            Some("this pane has no label of its own · label it in Rex to favorite it")
        );
        let mut empty = crate::tui::app::testkit::app();
        empty.selected = None;
        assert_eq!(
            key_at(&mut empty, '*', Instant::now()),
            None,
            "no row, nothing to do"
        );
    }

    #[test]
    fn with_no_marked_row_the_quote_says_so_and_goes_nowhere() {
        let mut app = crate::tui::app::testkit::app();
        let before = selected(&app);
        let effect = key_at(&mut app, '\'', Instant::now());
        assert_eq!(effect, None);
        assert_eq!(
            app.toast(),
            Some("no marked row · * marks one, or see [[mark]] in config.toml")
        );
        assert_eq!(selected(&app), before);
        let mut app = marked();
        for c in "/zzz".chars() {
            key_at(&mut app, c, Instant::now());
        }
        app.handle_at(KeyEvent::from(KeyCode::Enter), Instant::now());
        key_at(&mut app, '\'', Instant::now());
        assert_eq!(
            app.toast(),
            Some("no marked row in this view"),
            "the rules mark rows but the filter hides them all: no pointer to the config"
        );
    }

    #[test]
    fn a_jump_still_running_keeps_the_quote_from_moving_the_selection() {
        let mut app = marked();
        app.going = Some("going to somewhere…".into());
        assert_eq!(key_at(&mut app, '\'', Instant::now()), None);
        assert_eq!(app.toast(), Some("a jump or close is already running"));
        assert_eq!(selected(&app), "mini/seat-b", "unchanged");
    }

    #[test]
    fn in_pick_the_quote_picks_the_first_marked_row_and_again_the_next() {
        let t = Instant::now();
        let mut app = marked();
        app.pick = true;
        assert_eq!(
            jumped(key_at(&mut app, '\'', t)).as_deref(),
            Some("mini/seat-b")
        );
        app.going = None;
        let again = key_at(&mut app, '\'', t + Duration::from_millis(300));
        assert_eq!(jumped(again).as_deref(), Some("mini/codex"));
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
