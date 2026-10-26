use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use super::answer::{Answer, Step};
use super::columns::{Columns, Sort, by_letter};
use super::screen::Screen;
use super::status::Status;
use super::views::{Bucket, View, arrange, bucket, views};
use crate::board::{Group, Row};
use crate::output::{labels, name, path};
use crate::pane::Pane;
use crate::send::{Action, Expect};

const TOAST: Duration = Duration::from_secs(3);

const HIDDEN: &str = "the card is hidden · c shows it";
/// A blocked row's tail is read once its card has shown this long, so j/k across rows reads none.
const SETTLE: Duration = Duration::from_millis(300);
/// `tab` off a done row whose card showed at least this long marks it seen.
const READ: Duration = Duration::from_secs(1);

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
        session: String,
        block: String,
    },
    /// Read a pane for answer mode: its tail, and its input line with styles.
    Read {
        key: String,
        host: String,
        session: String,
        block: String,
    },
    /// Answer a pane, if it still shows `expect`; `what` names the answer.
    Send {
        key: String,
        pane: Box<Pane>,
        expect: Expect,
        action: Action,
        what: String,
    },
    /// Mark these unseen `done` rows seen.
    SeenAll(Vec<String>),
    /// Go to this row's pane.
    Jump(Box<Row>),
    /// Keep whether the card is hidden in tui.json.
    SavePrefs(super::prefs::Prefs),
    Reconnect,
    Quit,
}

/// An answer the board sent, which the card's bar names.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub key: String,
    /// `sent 1 (Yes) → seat-b 11:22:04`
    pub text: String,
    /// The report the answer was checked against.
    pub report: Option<String>,
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
    /// When the capture came back.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    /// The row's report when the tail was asked for; a later one makes it stale.
    pub updated_at: Option<String>,
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
    /// `f` and a letter: only rows with that status show.
    pub status: Option<Status>,
    /// `f` was pressed and waits for its letter.
    pub picking_status: bool,
    /// The `:` prompt: a view name being typed.
    pub command: Input,
    pub commanding: bool,
    pub view: View,
    pub sort: Option<Sort>,
    told_ctrl_s: bool,
    told_table: bool,
    /// Where the list's rows were last drawn: the first row's line, the first item shown, and
    /// how many rows fit. The draw sets it, so a click can find the row under it.
    pub drawn: std::cell::Cell<(u16, usize, usize)>,
    pub folded: BTreeSet<Bucket>,
    pub peek: Option<Peek>,
    /// The `?` key table is open in the card.
    pub keys: bool,
    /// `space`: the card takes the whole body.
    pub zoom: bool,
    /// The row whose card shows, and since when: `tab` reads it, and a blocked row's tail waits
    /// for it to settle.
    pub shown: Option<(String, Instant)>,
    /// The row and report the last automatic tail was for, so each is read once.
    tailed: Option<(String, Option<String>)>,
    told_angles: bool,
    pub toast: Option<(String, Instant)>,
    pub link: Link,
    /// `rexboard pick`: the list alone; enter goes and esc gives up.
    pub pick: bool,
    /// `c` hid the card, from tui.json.
    pub hidden: bool,
    /// The terminal's size at the last draw, which `<` and `>` move the pane against and a
    /// click is read against.
    pub width: u16,
    pub height: u16,
    /// `going to …` while a jump runs; a second Enter starts nothing.
    pub going: Option<String>,
    /// Why the last jump failed, until the next key.
    pub failed: Option<String>,
    needs: BTreeSet<String>,
    /// `a`: answering one row; the board's keys are off.
    pub answer: Option<Answer>,
    /// The last answer that went out, until its row needs you again.
    pub sent: Option<Sent>,
    /// The board's own pane (`REX_BLOCK`), which it never answers.
    pub own: Option<String>,
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
        // A row that needs you again with a newer report asks something new; the old answer
        // must not sit on it.
        if let Some(sent) = &self.sent {
            let asks_again = |row: &Row| {
                row.group == Group::NeedsYou
                    && newer(row.pane.updated_at.as_deref(), sent.report.as_deref())
            };
            let stands = self
                .rows
                .iter()
                .find(|row| row.key == sent.key)
                .is_some_and(|row| !asks_again(row));
            if !stands {
                self.sent = None;
            }
        }
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

    /// Whether the row passes the status filter and the `/` text.
    fn matches(&self, row: &Row) -> bool {
        self.status.is_none_or(|status| status.matches(row)) && self.matches_text(row)
    }

    /// Rows the `/` text keeps, with any status; the counts `f` shows.
    pub fn status_count(&self, status: Status) -> usize {
        self.rows
            .iter()
            .filter(|row| status.matches(row) && self.matches_text(row))
            .count()
    }

    fn matches_text(&self, row: &Row) -> bool {
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
        if self.clicks() && !self.told_table {
            self.told_table = true;
            self.say("click a header to sort · hold Option (iTerm2) or Shift to select text");
        }
        // A row in a group this view keeps folded rests on that group's header, as `z` does.
        if let Some(group) = self.current().and_then(|row| bucket(&self.view, row))
            && self.folded.contains(&group)
        {
            self.selected = Some(Spot::Group(group));
        }
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
        let typed = match self.command.value().trim() {
            // `flat` was the table view's name before v0.3.
            "flat" => "table".to_owned(),
            typed => typed.to_owned(),
        };
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
        // ponytail: one hint per run for S's move to ctrl+s; drop it in v0.3.
        if letter == 'S' && !self.told_ctrl_s {
            self.told_ctrl_s = true;
            self.say("sorted by state · bulk seen is ctrl+s now");
        }
        self.sort = match self.sort {
            Some((id, dir)) if id == column.id && dir == column.first => Some((id, dir.flip())),
            Some((id, _)) if id == column.id => None,
            _ => Some((column.id, column.first)),
        };
    }

    /// Whether clicks sort: in the table view, which turns mouse capture on, and not in pick.
    pub fn clicks(&self) -> bool {
        self.view.group_by.is_none() && !self.pick
    }

    /// Whether the card shows under the list: unless `c` hid it, and always while it is zoomed or
    /// holds the key table or a failed jump's reason, which only it shows whole. `pick` has none.
    pub fn card_shown(&self) -> bool {
        !self.pick
            && (!self.hidden
                || self.keys
                || self.zoom
                || self.failed.is_some()
                || self.answer.is_some())
    }

    fn prefs(&self) -> super::prefs::Prefs {
        super::prefs::Prefs {
            detail_hidden: self.hidden,
        }
    }

    /// Notes which row's card shows from `now`; the board calls it after every change.
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
    }

    /// A tail for a blocked row whose card has shown for SETTLE, once per report.
    pub fn auto_tail(&mut self, now: Instant) -> Option<Effect> {
        let (key, since) = self.shown.as_ref().filter(|_| self.answer.is_none())?;
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

    /// Asks for the current row's tail; the card shows it when it comes.
    fn tail(&mut self) -> Option<Effect> {
        let row = self.current()?;
        let effect = Effect::Peek {
            key: row.key.clone(),
            host: row.pane.host.clone(),
            session: row.pane.session_id.clone(),
            block: row.pane.block_id.clone(),
        };
        self.peek = Some(Peek {
            key: row.key.clone(),
            lines: None,
            at: None,
            updated_at: row.pane.updated_at.clone(),
        });
        Some(effect)
    }

    /// `tab`: a done row read for READ is seen; then on to the next NEEDS YOU row, wrapping.
    fn next_waiting(&mut self, now: Instant) -> Option<Effect> {
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
        let next = at.map_or(0, |at| at + 1) % waiting.len().max(1);
        match waiting.get(next) {
            Some(spot) => self.selected = Some(spot.clone()),
            None => self.say("nobody needs you"),
        }
        seen
    }

    /// A click at `x`, `y` in the table view: on a column's header cell it sorts as that
    /// column's letter does, on its arrow it flips the sort, and on a row it selects the row.
    pub fn click(&mut self, x: u16, y: u16) {
        use super::view::{HEADED, HEADER_ROW};
        let items = self.items();
        let list = usize::from(self.width);
        if !self.clicks() || self.prompting() || items.is_empty() || self.zoom {
            return;
        }
        if self.height >= HEADED && y == HEADER_ROW {
            let columns = Columns::new(self.width, list);
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

    /// Whether a prompt (`/`, `:` or `f`) has the keyboard, so the mouse leaves the list alone.
    fn prompting(&self) -> bool {
        self.filtering || self.commanding || self.picking_status || self.answer.is_some()
    }

    /// The wheel moves the selection, as `k` and `j` do, but stops at the ends; with the mouse
    /// captured the terminal no longer turns it into arrow keys.
    pub fn scroll(&mut self, up: bool) {
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

    pub fn handle(&mut self, key: KeyEvent) -> Option<Effect> {
        self.failed = None;
        if self.answer.is_some() {
            return self.answer_key(key, Instant::now());
        }
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
        // A key with ctrl or alt after `f` keeps its own meaning, so ctrl+c still quits.
        if self.picking_status && !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
            self.picking_status = false;
        }
        if self.picking_status {
            self.picking_status = false;
            match key.code {
                KeyCode::Char('f') | KeyCode::Esc => self.status = None,
                KeyCode::Char(c) => match Status::from_letter(c) {
                    Some(status) => {
                        self.status = Some(status);
                        // Folded groups would hide the very rows asked for.
                        self.unfold_all();
                    }
                    None => self.say("f takes w d q p e u i; f or esc clears"),
                },
                _ => {}
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
            KeyCode::Char('f') => self.picking_status = true,
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
                self.zoom = false;
            }
            KeyCode::Esc if self.pick => return Some(Effect::Quit),
            KeyCode::Esc => {
                self.filter.reset();
                self.status = None;
                self.peek = None;
                self.keys = false;
                self.zoom = false;
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
                    .items()
                    .into_iter()
                    .filter_map(|item| match item {
                        Item::Row(i) if self.rows[i].group == Group::NeedsYou => {
                            Some(Spot::Row(self.rows[i].key.clone()))
                        }
                        _ => None,
                    })
                    .nth(n);
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
            KeyCode::Char(' ') if self.zoom => self.zoom = false,
            KeyCode::Char(' ') if !self.pick => {
                self.current()?;
                self.keys = false;
                self.zoom = true;
                return self.tail();
            }
            KeyCode::Tab if !self.pick => return self.next_waiting(Instant::now()),
            KeyCode::Char('a') if !self.pick => return self.start_answer(),
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
            KeyCode::Char('c') if !self.pick => {
                self.hidden = !self.hidden;
                if self.hidden {
                    self.say(HIDDEN);
                } else {
                    self.toast = None;
                }
                return Some(Effect::SavePrefs(self.prefs()));
            }
            // The card spans the board, so there is no width left to size; said once a run.
            KeyCode::Char('<' | '>') if !self.pick && !self.told_angles => {
                self.told_angles = true;
                self.say("the card spans the board now, so < and > are gone · c hides it");
            }
            _ => {}
        }
        None
    }

    /// `a`: answer mode on the current row, once a fresh read of its pane comes back.
    fn start_answer(&mut self) -> Option<Effect> {
        let Some(row) = self.current() else {
            self.say("select a row to answer it");
            return None;
        };
        if row.pane.host == "local" && self.own.as_deref() == Some(row.pane.block_id.as_str()) {
            self.say("that is the board's own pane");
            return None;
        }
        let Some(updated_at) = row.pane.updated_at.clone() else {
            self.say("this pane reports no status, so it cannot be answered here · enter jumps");
            return None;
        };
        let row = row.clone();
        self.zoom = false;
        self.keys = false;
        let effect = read(&row);
        self.answer = Some(Answer::new(row, updated_at));
        Some(effect)
    }

    fn answer_key(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        let answer = self.answer.as_mut()?;
        match answer.key(key, now) {
            Step::Stay => None,
            Step::Leave => {
                self.answer = None;
                None
            }
            Step::Next => {
                self.answer = None;
                self.next_waiting(now)
            }
            Step::Send(action) => {
                let expect = answer.expect()?;
                let what = answer.describe(&action);
                answer.sending = Some(what.clone());
                Some(Effect::Send {
                    key: answer.row.key.clone(),
                    pane: Box::new(answer.row.pane.clone()),
                    expect,
                    action,
                    what,
                })
            }
        }
    }

    /// A read for answer mode came back.
    pub fn screen(&mut self, key: &str, result: Result<Screen, String>) {
        let Some(answer) = self.answer.as_mut().filter(|a| a.row.key == key) else {
            return;
        };
        match result {
            Ok(screen) => {
                // Text typed for an input line must never meet a dialog.
                if !screen.options.is_empty() {
                    answer.input.reset();
                }
                answer.shown.set(0);
                answer.screen = Some(screen);
                answer.captured = Some(chrono::Utc::now());
            }
            Err(why) => {
                answer.screen = Some(Screen::default());
                answer.error = Some(why);
            }
        }
    }

    /// A send finished. Sent: back to the board, with the card's bar saying so. Refused: stay,
    /// with the reason, and read the pane again, against the row's latest report.
    pub fn sent(&mut self, key: &str, what: &str, result: Result<(), String>) -> Option<Effect> {
        let latest = self.rows.iter().find(|row| row.key == key).cloned();
        let answering = self.answer.as_ref().is_some_and(|a| a.row.key == key);
        let why = match result {
            Ok(()) => {
                let report = match self.answer.take_if(|a| a.row.key == key) {
                    Some(answer) => Some(answer.updated_at),
                    None => latest.as_ref().and_then(|row| row.pane.updated_at.clone()),
                };
                let to = latest.as_ref().map_or_else(|| key.to_owned(), name);
                let at = chrono::Local::now().format("%H:%M:%S");
                self.sent = Some(Sent {
                    key: key.to_owned(),
                    text: format!("sent {what} → {to} {at}"),
                    report,
                });
                return None;
            }
            Err(why) if !answering => {
                self.say(why);
                return None;
            }
            Err(why) => why,
        };
        let answer = self.answer.as_mut()?;
        if let Some(row) = latest
            && let Some(at) = row.pane.updated_at.clone()
        {
            answer.updated_at = at;
            answer.row = row;
        }
        answer.sending = None;
        answer.input.reset();
        answer.armed = None;
        answer.screen = None;
        answer.captured = None;
        answer.error = Some(why);
        Some(read(&answer.row))
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

/// Whether report `a` came after `b`.
fn newer(a: Option<&str>, b: Option<&str>) -> bool {
    let at = |s: Option<&str>| s.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
    match (at(a), at(b)) {
        (Some(a), Some(b)) => a > b,
        _ => a != b,
    }
}

fn read(row: &Row) -> Effect {
    Effect::Read {
        key: row.key.clone(),
        host: row.pane.host.clone(),
        session: row.pane.session_id.clone(),
        block: row.pane.block_id.clone(),
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

    const DIALOG: &str = " ❯ 1. Yes\n   2. Yes, and don't ask again\n   3. No\n Esc to cancel\n";

    /// `a` on the selected row, with its pane read back as `text`.
    fn answering(app: &mut App, text: &str) -> String {
        let Some(Effect::Read { key, .. }) = press(app, KeyCode::Char('a')) else {
            panic!("a reads the pane first");
        };
        app.screen(&key, Ok(Screen::parse(text, None)));
        drawn(app);
        key
    }

    /// The card drawn as the board draws it, which tells answer mode the options on screen.
    fn drawn(app: &App) {
        if let Some(answer) = &app.answer {
            answer.lines(chrono::Utc::now(), 120, 40);
        }
    }

    #[test]
    fn answer_a_reads_the_pane_then_a_digit_sends_to_the_row_it_was_opened_on() {
        let mut app = app();
        assert_eq!(
            press(&mut app, KeyCode::Char('a')),
            Some(Effect::Read {
                key: "mini/seat-b".into(),
                host: "mini".into(),
                session: "session:api".into(),
                block: "seat-b".into(),
            })
        );
        assert_eq!(
            press(&mut app, KeyCode::Char('1')),
            None,
            "waits for the read"
        );
        app.screen("mini/seat-b", Ok(Screen::parse(DIALOG, None)));
        drawn(&app);
        let mut moved = rows();
        moved.reverse();
        app.set_rows(moved, hosts());
        let Some(Effect::Send {
            key,
            pane,
            expect,
            action,
            what,
        }) = press(&mut app, KeyCode::Char('1'))
        else {
            panic!("1 sends");
        };
        assert_eq!((key.as_str(), pane.host.as_str()), ("mini/seat-b", "mini"));
        assert_eq!(action, Action::Pick('1'));
        assert_eq!(what, "1 (Yes)");
        assert_eq!(expect.updated_at, "2026-01-01T00:00:00Z");
        assert!(expect.fingerprint.starts_with("\n ❯ 1. Yes"));
        assert_eq!(expect.title, None, "a native row has no title check");
    }

    #[test]
    fn answer_board_keys_are_off_and_ctrl_c_leaves_without_quitting() {
        let mut app = app();
        answering(&mut app, "────\n❯\u{a0}\n");
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('s'),
            KeyCode::Char('1'),
            KeyCode::Char('f'),
            KeyCode::Char(':'),
            KeyCode::Char('q'),
        ] {
            assert_eq!(press(&mut app, code), None, "{code:?}");
        }
        assert_eq!(selected(&app), "mini/seat-b");
        assert!(!app.filtering && !app.picking_status && !app.commanding);
        assert_eq!(app.answer.as_ref().unwrap().input.value(), "js1f:q");
        press(&mut app, KeyCode::Esc);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle(ctrl_c), None, "not Quit");
        assert!(app.answer.is_none());
        assert_eq!(
            app.handle(ctrl_c),
            Some(Effect::Quit),
            "on the board it quits"
        );
    }

    #[test]
    fn answer_tab_leaves_for_the_next_row_that_needs_you() {
        let mut app = app();
        answering(&mut app, DIALOG);
        assert_eq!(press(&mut app, KeyCode::Tab), None);
        assert!(app.answer.is_none());
        assert_eq!(selected(&app), "local/claude");
    }

    #[test]
    fn answer_is_refused_on_the_boards_own_pane_and_a_pane_with_no_record() {
        let mut app = App::default();
        let mut quiet = row("local/plain", "idle", "needs_you", false);
        quiet.pane.updated_at = None;
        app.set_rows(
            vec![row("local/board", "idle", "needs_you", false), quiet],
            hosts(),
        );
        app.own = Some("board".into());
        assert_eq!(press(&mut app, KeyCode::Char('a')), None);
        assert_eq!(app.toast(), Some("that is the board's own pane"));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(press(&mut app, KeyCode::Char('a')), None);
        assert!(
            app.toast()
                .unwrap()
                .starts_with("this pane reports no status")
        );
        assert!(app.answer.is_none());
    }

    #[test]
    fn answer_a_refusal_keeps_the_mode_and_reads_again_against_the_newest_report() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        let mut newer = rows();
        newer[0].pane.updated_at = Some("2026-01-01T00:00:09Z".into());
        app.set_rows(newer, hosts());
        let again = app.sent(
            &key,
            "1 (Yes)",
            Err("the agent's status changed since it was read; nothing sent".into()),
        );
        assert!(matches!(again, Some(Effect::Read { .. })), "{again:?}");
        let answer = app.answer.as_ref().expect("still answering");
        assert_eq!(
            answer.error.as_deref(),
            Some("the agent's status changed since it was read; nothing sent")
        );
        assert_eq!(answer.updated_at, "2026-01-01T00:00:09Z");
        assert!(answer.screen.is_none() && answer.sending.is_none());
    }

    #[test]
    fn answer_text_typed_for_an_idle_prompt_never_reaches_a_dialog_that_replaced_it() {
        let mut app = app();
        let key = answering(&mut app, "────\n❯\u{a0}\n");
        for c in "go ahead".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Send {
                action: Action::Text(_),
                ..
            })
        ));
        let why = "the agent's status changed since it was read; nothing sent";
        assert!(matches!(
            app.sent(&key, "\"go ahead\"", Err(why.into())),
            Some(Effect::Read { .. })
        ));
        assert_eq!(
            app.answer.as_ref().unwrap().input.value(),
            "",
            "a refusal clears it"
        );
        app.screen(&key, Ok(Screen::parse(DIALOG, None)));
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        for c in "go ahead".chars() {
            assert_eq!(press(&mut app, KeyCode::Char(c)), None);
        }
        assert_eq!(
            app.answer.as_ref().unwrap().error.as_deref(),
            Some("pick a number; this dialog takes no text")
        );
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            None,
            "no text into a dialog"
        );
    }

    #[test]
    fn answer_the_sent_line_lasts_until_the_row_needs_you_again() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        app.sent(&key, "1 (Yes)", Ok(()));
        let report = |app: &mut App, state: &str, group: &str, at: &str| {
            let mut all = rows();
            all[0] = row("mini/seat-b", state, group, false);
            all[0].pane.updated_at = Some(at.into());
            app.set_rows(all, hosts());
        };
        report(&mut app, "blocked", "needs_you", "2026-01-01T00:00:00Z");
        assert!(app.sent.is_some(), "the same report: kept");
        report(&mut app, "working", "working", "2026-01-01T00:00:05Z");
        assert!(
            app.sent.is_some(),
            "kept while the agent works on the answer"
        );
        report(&mut app, "blocked", "needs_you", "2026-01-01T00:01:00Z");
        assert_eq!(app.sent, None, "a new ask drops it");
    }

    #[test]
    fn answer_sent_returns_to_the_board_on_the_same_row_and_says_what_went() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.sent(&key, "1 (Yes)", Ok(())), None);
        assert!(app.answer.is_none());
        assert_eq!(selected(&app), "mini/seat-b");
        let sent = app.sent.clone().unwrap();
        assert_eq!(sent.key, "mini/seat-b");
        assert_eq!(sent.report.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert!(
            sent.text.starts_with("sent 1 (Yes) → seat-b "),
            "{}",
            sent.text
        );
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
