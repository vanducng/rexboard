use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use super::columns::{Sort, by_letter};
use super::views::{Bucket, View, arrange, bucket, views};
use crate::board::{Group, Row};
use crate::output::{labels, path};

const TOAST: Duration = Duration::from_secs(3);

/// What a key asks the outside world to do.
#[derive(Debug, PartialEq)]
pub enum Effect {
    Seen {
        key: String,
        label: String,
        state: String,
    },
    Peek {
        key: String,
        host: String,
        block: String,
    },
    /// Mark these unseen `done` rows seen.
    SeenAll(Vec<String>),
    /// Go to this row's pane.
    Jump(Box<Row>),
    Reconnect,
    Quit,
}

/// One line of the list: a group header or a row (an index into `App::rows`).
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Header {
        bucket: Bucket,
        count: usize,
        folded: bool,
    },
    Row(usize),
}

/// What the selection rests on: a row, or a folded group's header so `z` can open it again.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    Row(String),
    Group(Bucket),
}

pub struct Peek {
    pub key: String,
    /// None while capturing; the lines, or why the capture failed.
    pub lines: Option<Result<Vec<String>, String>>,
}

/// Where the board stands with serve.
#[derive(Debug, Default, Clone, PartialEq)]
pub enum Link {
    /// No rows yet: serve may still be starting.
    #[default]
    Connecting,
    Up,
    /// The connection dropped or never came up, and why.
    Lost(String),
}

#[derive(Default)]
pub struct App {
    /// Agent rows in board order, as serve sends them.
    pub rows: Vec<Row>,
    pub panes: usize,
    pub hosts: BTreeMap<String, String>,
    pub selected: Option<Spot>,
    pub filter: Input,
    pub filtering: bool,
    /// The `:` prompt: a view name being typed.
    pub command: Input,
    pub commanding: bool,
    pub view: View,
    pub sort: Option<Sort>,
    pub folded: BTreeSet<Bucket>,
    pub peek: Option<Peek>,
    /// The `?` key table is open in the detail pane.
    pub keys: bool,
    pub toast: Option<(String, Instant)>,
    pub link: Link,
    /// `rexboard pick`: the list alone; enter goes and esc gives up.
    pub pick: bool,
    /// `going to …` while a jump runs; a second Enter starts nothing.
    pub going: Option<String>,
    /// Why the last jump failed, until the next key.
    pub failed: Option<String>,
    needs: BTreeSet<String>,
}

impl App {
    /// New rows from serve. The selection follows its key; a row that newly needs you opens
    /// NEEDS YOU if it was folded.
    pub fn set_rows(&mut self, all: Vec<Row>, hosts: BTreeMap<String, String>) {
        self.panes = all.len();
        self.rows = all.into_iter().filter(|row| row.pane.is_agent()).collect();
        self.hosts = hosts;
        self.link = Link::Up;
        let needs: BTreeSet<String> = self
            .rows
            .iter()
            .filter(|row| row.group == Group::NeedsYou)
            .map(|row| row.key.clone())
            .collect();
        if !needs.is_subset(&self.needs) {
            self.folded.remove(&Bucket::Status(Group::NeedsYou));
        }
        self.needs = needs;
        self.keep_selection_visible();
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// A jump finished: say where it went, or keep why it failed in view.
    pub fn jumped(&mut self, result: Result<String, String>) {
        self.going = None;
        self.toast = None;
        match result {
            Ok(text) => self.say(text),
            Err(why) => self.failed = Some(why),
        }
    }

    pub fn toast(&self) -> Option<&str> {
        self.toast
            .as_ref()
            .filter(|(_, at)| at.elapsed() < TOAST)
            .map(|(text, _)| text.as_str())
    }

    fn matches(&self, row: &Row) -> bool {
        let query = self.filter.value().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let p = &row.pane;
        [&p.host, &p.session, &p.tab, &p.pane, &p.title, &p.state]
            .into_iter()
            .chain(p.msg.as_ref())
            .chain(p.harness.as_ref())
            .any(|field| field.to_lowercase().contains(&query))
    }

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
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) => Some(Spot::Row(self.rows[i].key.clone())),
                Item::Header {
                    bucket,
                    folded: true,
                    ..
                } => Some(Spot::Group(bucket)),
                Item::Header { .. } => None,
            })
            .collect()
    }

    pub fn current(&self) -> Option<&Row> {
        let Some(Spot::Row(key)) = &self.selected else {
            return None;
        };
        self.rows.iter().find(|row| &row.key == key)
    }

    /// Moves the selection, wrapping at both ends.
    fn step(&mut self, by: isize) {
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

    fn first_row_of(&self, group: &Bucket) -> Option<Spot> {
        self.items().into_iter().find_map(|item| match item {
            Item::Row(i) if bucket(&self.view, &self.rows[i]).as_ref() == Some(group) => {
                Some(Spot::Row(self.rows[i].key.clone()))
            }
            _ => None,
        })
    }

    fn unfold_all(&mut self) {
        self.folded.clear();
        if let Some(Spot::Group(group)) = self.selected.clone() {
            self.selected = self.first_row_of(&group);
        }
    }

    /// Switches to `view`; the selection keeps its row.
    fn show(&mut self, view: View) {
        if let Some(Spot::Group(_)) = self.selected {
            self.unfold_all();
        }
        self.view = view;
        self.keep_selection_visible();
    }

    /// The `:` prompt's Tab: the one view name the typed text starts, else the longest prefix
    /// the names share.
    fn complete(&mut self) {
        let typed = self.command.value().to_owned();
        let names: Vec<String> = views()
            .into_iter()
            .map(|v| v.name)
            .filter(|name| name.starts_with(&typed))
            .collect();
        let Some(first) = names.first() else {
            return;
        };
        let common = names.iter().fold(first.clone(), |acc, name| {
            acc.chars()
                .zip(name.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect()
        });
        self.command = Input::new(common);
    }

    /// The `:` prompt's Enter: a view's name, or the start of exactly one.
    fn open(&mut self) {
        let typed = self.command.value().trim().to_owned();
        let all = views();
        let found = all.iter().find(|v| v.name == typed).cloned().or_else(|| {
            let mut starts = all.iter().filter(|v| v.name.starts_with(&typed));
            match (starts.next(), starts.next()) {
                (Some(one), None) if !typed.is_empty() => Some(one.clone()),
                _ => None,
            }
        });
        match found {
            Some(view) => self.show(view),
            None => self.say(format!("no view \"{typed}\"")),
        }
    }

    /// Shift and a column's letter: sort by it, or flip it if it is already the sort.
    fn sort_by(&mut self, letter: char) {
        let Some(column) = by_letter(letter) else {
            return;
        };
        self.sort = Some(match self.sort {
            Some((id, dir)) if id == column.id => (id, dir.flip()),
            _ => (column.id, column.first),
        });
    }

    pub fn handle(&mut self, key: KeyEvent) -> Option<Effect> {
        self.failed = None;
        if self.commanding {
            match key.code {
                KeyCode::Esc => self.commanding = false,
                KeyCode::Tab => self.complete(),
                KeyCode::Enter => {
                    self.commanding = false;
                    self.open();
                }
                _ => {
                    self.command.handle_event(&Event::Key(key));
                }
            }
            return None;
        }
        if self.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter.reset();
                }
                KeyCode::Enter => self.filtering = false,
                _ => {
                    self.filter.handle_event(&Event::Key(key));
                }
            }
            self.keep_selection_visible();
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Some(Effect::Quit),
                KeyCode::Char('s') => self.seen_all(),
                _ => None,
            };
        }
        match key.code {
            KeyCode::Char('q') => return Some(Effect::Quit),
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Char('g') | KeyCode::Home => self.selected = self.spots().first().cloned(),
            KeyCode::Char('G') | KeyCode::End => self.selected = self.spots().last().cloned(),
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Char(':') => {
                self.command.reset();
                self.commanding = true;
            }
            KeyCode::Char('v') => {
                let all = views();
                let at = all.iter().position(|v| *v == self.view).unwrap_or(0);
                self.show(all[(at + 1) % all.len()].clone());
            }
            KeyCode::Char(c) if by_letter(c).is_some() => self.sort_by(c),
            KeyCode::Char('?') => {
                self.keys = !self.keys;
                self.peek = None;
            }
            KeyCode::Esc if self.pick => return Some(Effect::Quit),
            KeyCode::Esc => {
                self.filter.reset();
                self.peek = None;
                self.keys = false;
                self.sort = None;
                self.unfold_all();
                self.keep_selection_visible();
            }
            KeyCode::Char('z') => match self.selected.clone() {
                Some(Spot::Group(group)) => {
                    self.folded.remove(&group);
                    self.selected = self.first_row_of(&group);
                }
                Some(Spot::Row(_)) => {
                    if let Some(group) = self.current().and_then(|row| bucket(&self.view, row)) {
                        self.folded.insert(group.clone());
                        self.selected = Some(Spot::Group(group));
                    }
                }
                None => {}
            },
            KeyCode::Char(c @ '1'..='9') => {
                for row in self.rows.iter().filter(|row| row.group == Group::NeedsYou) {
                    if let Some(group) = bucket(&self.view, row) {
                        self.folded.remove(&group);
                    }
                }
                let n = c as usize - '1' as usize;
                let pick = self
                    .rows
                    .iter()
                    .filter(|row| row.group == Group::NeedsYou && self.matches(row))
                    .nth(n)
                    .map(|row| Spot::Row(row.key.clone()));
                if pick.is_some() {
                    self.selected = pick;
                }
            }
            KeyCode::Char('s') => {
                return self.current().map(|row| Effect::Seen {
                    key: row.key.clone(),
                    label: path(&labels(row, true), usize::MAX),
                    state: row.pane.state.clone(),
                });
            }
            KeyCode::Char(' ') => {
                let row = self.current()?;
                let (key, host, block) = (
                    row.key.clone(),
                    row.pane.host.clone(),
                    row.pane.block_id.clone(),
                );
                if self.peek.as_ref().is_some_and(|peek| peek.key == key) {
                    self.peek = None;
                    return None;
                }
                self.keys = false;
                self.peek = Some(Peek {
                    key: key.clone(),
                    lines: None,
                });
                return Some(Effect::Peek { key, host, block });
            }
            KeyCode::Enter if self.going.is_some() => self.say("a jump is already running"),
            KeyCode::Enter => {
                let row = self.current()?.clone();
                self.going = Some(format!(
                    "going to {}…",
                    path(&labels(&row, true), usize::MAX)
                ));
                return Some(Effect::Jump(Box::new(row)));
            }
            KeyCode::Char('r') => {
                self.link = Link::Connecting;
                return Some(Effect::Reconnect);
            }
            _ => {}
        }
        None
    }

    /// `ctrl+s`: every unseen `done` row the list shows, to be marked seen.
    fn seen_all(&mut self) -> Option<Effect> {
        let keys: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.pane.state == "done" && !row.seen && self.matches(row))
            .map(|row| row.key.clone())
            .collect();
        if keys.is_empty() {
            self.say("no unseen done rows");
            return None;
        }
        Some(Effect::SeenAll(keys))
    }

    fn keep_selection_visible(&mut self) {
        let spots = self.spots();
        if !self
            .selected
            .as_ref()
            .is_some_and(|spot| spots.contains(spot))
        {
            self.selected = spots.first().cloned();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tui::columns::{Col, Dir};

    pub fn row(key: &str, state: &str, group: &str, seen: bool) -> Row {
        let (host, block) = key.split_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "key": key, "host": host, "session_id": "session:api", "session": "api",
            "window_id": "window:main", "tab": "main", "block_id": block, "pane": block,
            "title": format!("{block} title"), "harness": "claude-code", "state": state,
            "kind": null, "msg": format!("{block} msg"), "updated_at": "2026-01-01T00:00:00Z",
            "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
            "since_estimated": false, "turn_started": null, "last_turn": null,
            "seen": seen, "stale": false, "suspect": false, "suspect_reason": null,
            "group": group
        }))
        .unwrap()
    }

    pub fn hosts() -> BTreeMap<String, String> {
        BTreeMap::from([("local".into(), "up".into()), ("mini".into(), "up".into())])
    }

    pub fn rows() -> Vec<Row> {
        vec![
            row("mini/seat-b", "blocked", "needs_you", false),
            row("local/claude", "done", "needs_you", false),
            row("mini/codex", "working", "working", true),
            row("local/seat-a", "idle", "quiet", true),
        ]
    }

    pub fn app() -> App {
        let mut app = App::default();
        app.set_rows(rows(), hosts());
        app
    }

    pub fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
        app.handle(KeyEvent::from(code))
    }

    fn ctrl_s(app: &mut App) -> Option<Effect> {
        app.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
    }

    fn selected(app: &App) -> String {
        match app.selected.clone().unwrap() {
            Spot::Row(key) => key,
            Spot::Group(group) => format!("{group:?}"),
        }
    }

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
    fn filter_narrows_across_groups() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        for c in "mini".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let header = |group, count| Item::Header {
            bucket: Bucket::Status(group),
            count,
            folded: false,
        };
        assert_eq!(
            app.items(),
            [
                header(Group::NeedsYou, 1),
                Item::Row(0),
                header(Group::Working, 1),
                Item::Row(2)
            ]
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.items().len(), 7, "esc clears the filter");
    }

    #[test]
    fn ctrl_s_marks_only_unseen_done_rows_and_capital_s_sorts() {
        let mut app = App::default();
        let mut all = rows();
        all.push(row("mini/old-done", "done", "needs_you", false));
        all.push(row("mini/seen-done", "done", "quiet", true));
        all.push(row("mini/broken", "error", "needs_you", false));
        app.set_rows(all, hosts());
        assert_eq!(press(&mut app, KeyCode::Char('S')), None, "S sorts now");
        assert_eq!(app.sort, Some((Col::State, Dir::Up)));
        assert_eq!(
            ctrl_s(&mut app),
            Some(Effect::SeenAll(vec![
                "local/claude".into(),
                "mini/old-done".into()
            ])),
            "never blocked, error or already seen rows"
        );
        press(&mut app, KeyCode::Char('/'));
        for c in "old".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            ctrl_s(&mut app),
            Some(Effect::SeenAll(vec!["mini/old-done".into()])),
            "a kept filter narrows it to the rows shown"
        );
        press(&mut app, KeyCode::Esc);
        app.set_rows(
            vec![row("mini/seat-b", "blocked", "needs_you", false)],
            hosts(),
        );
        assert_eq!(ctrl_s(&mut app), None);
        assert_eq!(app.toast(), Some("no unseen done rows"));
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
        assert_eq!(app.toast(), Some("a jump is already running"));
        app.jumped(Err("the Rex app on studio did not answer within 5s".into()));
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
        app.jumped(Ok("jumped to api › claude on local".into()));
        assert_eq!(app.toast(), Some("jumped to api › claude on local"));
    }

    #[test]
    fn s_sends_seen_for_the_selected_key() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(
            press(&mut app, KeyCode::Char('s')),
            Some(Effect::Seen {
                key: "local/claude".into(),
                label: "api › claude".into(),
                state: "done".into(),
            })
        );
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Jump(row)) if row.key == "local/claude"
        ));
    }

    #[test]
    fn z_folds_then_unfolds_from_the_folded_header() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('z'));
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
        press(&mut app, KeyCode::Char('z'));
        assert!(app.folded.is_empty(), "z on a folded header opens it");
        assert_eq!(selected(&app), "mini/codex");
    }

    #[test]
    fn esc_unfolds_every_group_even_with_all_folded() {
        let mut app = app();
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('z'));
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.folded.len(), 3);
        assert!(
            app.spots()
                .iter()
                .all(|spot| matches!(spot, Spot::Group(_)))
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.folded.is_empty());
        assert!(app.current().is_some());
    }

    #[test]
    fn a_new_row_in_needs_you_unfolds_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('z'));
        assert!(app.folded.contains(&Bucket::Status(Group::NeedsYou)));
        app.set_rows(rows(), hosts());
        assert!(
            app.folded.contains(&Bucket::Status(Group::NeedsYou)),
            "same rows: stays folded"
        );
        let mut more = rows();
        more.insert(0, row("mini/seat-x", "blocked", "needs_you", false));
        app.set_rows(more, hosts());
        assert!(
            !app.folded.contains(&Bucket::Status(Group::NeedsYou)),
            "a new blocked row opens it"
        );
    }
}
