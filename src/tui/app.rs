use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::board::{Group, Row};

const TOAST: Duration = Duration::from_secs(3);

/// What a key asks the outside world to do.
#[derive(Debug, PartialEq)]
pub enum Effect {
    Seen(String),
    Peek {
        key: String,
        host: String,
        block: String,
    },
    Reconnect,
    Quit,
}

/// One line of the list: a group header or a row (an index into `App::rows`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    Header(Group, usize),
    Row(usize),
}

pub struct Peek {
    pub key: String,
    pub lines: Option<Vec<String>>,
}

#[derive(Default)]
pub struct App {
    /// Agent rows in board order, as serve sends them.
    pub rows: Vec<Row>,
    pub panes: usize,
    pub hosts: BTreeMap<String, String>,
    pub selected: Option<String>,
    pub filter: Input,
    pub filtering: bool,
    pub folded: BTreeSet<Group>,
    pub peek: Option<Peek>,
    pub toast: Option<(String, Instant)>,
    pub connected: bool,
}

impl App {
    /// New rows from serve. The selection follows its key, else lands on the first row.
    pub fn set_rows(&mut self, all: Vec<Row>, hosts: BTreeMap<String, String>) {
        self.panes = all.len();
        self.rows = all.into_iter().filter(|row| row.pane.is_agent()).collect();
        self.hosts = hosts;
        self.connected = true;
        let keep = self.selected.as_ref().is_some_and(|key| self.visible(key));
        if !keep {
            self.selected = self.rows_shown().first().map(|&i| self.rows[i].key.clone());
        }
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
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

    /// Headers and rows, filtered, with folded groups showing only their header.
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let group = self.rows[i].group;
            let members: Vec<usize> = (i..self.rows.len())
                .take_while(|&j| self.rows[j].group == group)
                .filter(|&j| self.matches(&self.rows[j]))
                .collect();
            i += (i..self.rows.len())
                .take_while(|&j| self.rows[j].group == group)
                .count();
            if members.is_empty() {
                continue;
            }
            items.push(Item::Header(group, members.len()));
            if !self.folded.contains(&group) {
                items.extend(members.into_iter().map(Item::Row));
            }
        }
        items
    }

    fn rows_shown(&self) -> Vec<usize> {
        self.items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Row(i) => Some(i),
                Item::Header(..) => None,
            })
            .collect()
    }

    fn visible(&self, key: &str) -> bool {
        self.rows_shown()
            .into_iter()
            .any(|i| self.rows[i].key == key)
    }

    pub fn current(&self) -> Option<&Row> {
        let key = self.selected.as_ref()?;
        self.rows.iter().find(|row| &row.key == key)
    }

    /// Moves the selection over rows only, wrapping at both ends.
    fn step(&mut self, by: isize) {
        let shown = self.rows_shown();
        if shown.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|key| shown.iter().position(|&i| &self.rows[i].key == key));
        let next = match at {
            None => 0,
            Some(at) => (at as isize + by).rem_euclid(shown.len() as isize) as usize,
        };
        self.selected = Some(self.rows[shown[next]].key.clone());
    }

    fn select_at(&mut self, pick: impl FnOnce(&[usize]) -> Option<usize>) {
        if let Some(i) = pick(&self.rows_shown()) {
            self.selected = Some(self.rows[i].key.clone());
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> Option<Effect> {
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
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Effect::Quit);
        }
        match key.code {
            KeyCode::Char('q') => return Some(Effect::Quit),
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Char('g') | KeyCode::Home => self.select_at(|s| s.first().copied()),
            KeyCode::Char('G') | KeyCode::End => self.select_at(|s| s.last().copied()),
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Esc => {
                self.filter.reset();
                self.peek = None;
                self.keep_selection_visible();
            }
            KeyCode::Char('z') => {
                if let Some(group) = self.current().map(|row| row.group) {
                    if !self.folded.remove(&group) {
                        self.folded.insert(group);
                    }
                    self.keep_selection_visible();
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let n = c as usize - '1' as usize;
                let pick = self
                    .rows_shown()
                    .into_iter()
                    .filter(|&i| self.rows[i].group == Group::NeedsYou)
                    .nth(n);
                if let Some(i) = pick {
                    self.selected = Some(self.rows[i].key.clone());
                }
            }
            KeyCode::Char('s') => return self.current().map(|row| Effect::Seen(row.key.clone())),
            KeyCode::Char(' ') => {
                let row = self.current()?;
                let effect = Effect::Peek {
                    key: row.key.clone(),
                    host: row.pane.host.clone(),
                    block: row.pane.block_id.clone(),
                };
                if self.peek.as_ref().is_some_and(|peek| peek.key == row.key) {
                    self.peek = None;
                    return None;
                }
                self.peek = Some(Peek {
                    key: row.key.clone(),
                    lines: None,
                });
                return Some(effect);
            }
            KeyCode::Enter => {
                if self.current().is_some() {
                    self.say("jump arrives in a later version");
                }
            }
            KeyCode::Char('r') => return Some(Effect::Reconnect),
            _ => {}
        }
        None
    }

    fn keep_selection_visible(&mut self) {
        if !self.selected.as_ref().is_some_and(|key| self.visible(key)) {
            self.selected = self.rows_shown().first().map(|&i| self.rows[i].key.clone());
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn row(key: &str, state: &str, group: &str, seen: bool) -> Row {
        let (host, block) = key.split_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "key": key, "host": host, "session_id": "session:api", "session": "api",
            "window_id": "window:main", "tab": "main", "block_id": block, "pane": block,
            "title": format!("{block} title"), "harness": "claude-code", "state": state,
            "kind": null, "msg": format!("{block} msg"), "updated_at": "2026-01-01T00:00:00Z",
            "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
            "since_estimated": false, "turn_started": null, "last_turn": null,
            "seen": seen, "stale": false, "group": group
        }))
        .unwrap()
    }

    pub fn app() -> App {
        let mut app = App::default();
        app.set_rows(
            vec![
                row("mini/seat-b", "blocked", "needs_you", false),
                row("local/claude", "done", "needs_you", false),
                row("mini/codex", "working", "working", true),
                row("local/seat-a", "idle", "quiet", true),
            ],
            BTreeMap::from([("local".into(), "up".into()), ("mini".into(), "up".into())]),
        );
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
        app.handle(KeyEvent::from(code))
    }

    fn selected(app: &App) -> &str {
        app.selected.as_deref().unwrap()
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
        let items = app.items();
        assert_eq!(
            items,
            [
                Item::Header(Group::NeedsYou, 1),
                Item::Row(0),
                Item::Header(Group::Working, 1),
                Item::Row(2)
            ]
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.items().len(), 7, "esc clears the filter");
    }

    #[test]
    fn s_sends_seen_for_the_selected_key() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(
            press(&mut app, KeyCode::Char('s')),
            Some(Effect::Seen("local/claude".into()))
        );
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.toast(), Some("jump arrives in a later version"));
    }
}
