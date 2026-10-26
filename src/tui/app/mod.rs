use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use unicode_width::UnicodeWidthStr;

use super::answer::{Answer, Step};
use super::columns::{Col, Column, Columns, Sort, by_letter, field_column, free_letter};
use super::full::Full;
use super::keys::{self, Act, K};
use super::screen::Screen;
use super::status::Status;
use super::views::{Bucket, Key, View, arrange, bucket, views};
use crate::board::{Group, Row};
use crate::client::{ExtRun, Field};
use crate::output::{clean, harness, labels, name, path, squeeze, wheres};
use crate::pane::Pane;
use crate::send::{Action, Expect};

mod filter;

const TOAST: Duration = Duration::from_secs(3);
/// Keys this close together are typing or a paste, not a person answering a question.
pub const GUARD: Duration = Duration::from_millis(300);
const TOO_FAST_TO_ANSWER: &str = "keys came too fast; nothing sent · a answers";
const TOO_FAST_TO_CLOSE: &str = "keys came too fast; nothing closed · dd closes";

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
        /// With the pane's colours, for the full view; the card reads plain text.
        vt: bool,
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
    /// `-`: go to the pane before the last jump.
    Back,
    /// Close this row's pane, which the operator confirmed.
    Close(Box<Row>),
    /// Put `text` on the clipboard; `shown` is how the footer names it.
    Copy {
        text: String,
        shown: String,
    },
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
    /// The enabled extensions' fields in `enabled` order, each with its sort letter.
    pub fields: Vec<(Field, Option<char>)>,
    /// How each enabled extension's last run went.
    pub ext_runs: BTreeMap<String, ExtRun>,
    pub selected: Option<Spot>,
    pub filter: Input,
    pub filtering: bool,
    /// `f` and a letter: only rows with that status show.
    pub status: Option<Status>,
    /// A prefix key (`g o z y f d`) waiting for the key after it.
    pub pending: Option<char>,
    /// The `:` prompt: a view name being typed.
    pub command: Input,
    pub commanding: bool,
    pub view: View,
    pub sort: Option<Sort>,
    /// Keys whose meaning changed (`R`, `M`, `N`), told once a run.
    told: BTreeSet<char>,
    told_table: bool,
    /// Where the list's rows were last drawn: the first row's line, the first item shown, and
    /// how many rows fit. The draw sets it, so a click can find the row under it.
    pub drawn: std::cell::Cell<(u16, usize, usize)>,
    pub folded: BTreeSet<Bucket>,
    pub peek: Option<Peek>,
    /// The `?` key table is open in the card.
    pub keys: bool,
    /// `space`: one row's pane across the whole terminal, read again while it is open.
    pub full: Option<Full>,
    /// The row whose card shows, and since when: `tab` reads it, and a blocked row's tail waits
    /// for it to settle.
    pub shown: Option<(String, Instant)>,
    /// The row and report the last automatic tail was for, so each is read once.
    tailed: Option<(String, Option<String>)>,
    pub toast: Option<(String, Instant)>,
    pub link: Link,
    /// `rexboard pick`: the list alone; enter goes and esc gives up.
    pub pick: bool,
    /// `c` hid the card, from tui.json.
    pub hidden: bool,
    /// tui.json's motion, written back when `c` saves the file.
    pub motion: super::prefs::Motion,
    /// tui.json's icons, written back the same way.
    pub icons: super::prefs::Icons,
    /// Whether the card's facts take Nerd Font glyphs: REXBOARD_ICONS, else tui.json's icons.
    pub nerd: bool,
    /// The terminal's size at the last draw, which `<` and `>` move the pane against and a
    /// click is read against.
    pub width: u16,
    pub height: u16,
    /// `going to …` while a jump or a close runs; a second one starts nothing.
    pub going: Option<String>,
    /// Why the last jump or close failed, until the next key.
    pub failed: Option<String>,
    /// `dd`: the row to close once `y` confirms it, and when it asked.
    pub closing: Option<(Row, Instant)>,
    /// When answer mode opened, until its first key: a key sooner than GUARD is typing, and
    /// leaves it.
    answer_guard: Option<Instant>,
    /// When the last key came, and whether this one came a quiet GUARD after it: typing never
    /// does, so `dd` and `a`/`i` only act on a key pressed on its own.
    last_key: Option<Instant>,
    quiet: bool,
    /// Whether the waiting prefix's key came on its own.
    pending_quiet: bool,
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
        let viewed = self
            .full
            .as_ref()
            .and_then(|full| self.rows.iter().find(|row| row.key == full.key))
            .map(crate::output::name);
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
        if let Some((row, _)) = &self.closing
            && !self.rows.iter().any(|r| r.key == row.key)
        {
            self.say(format!(
                "{} left the board; nothing to close",
                path(&labels(row, false), usize::MAX)
            ));
            self.closing = None;
        }
        if let Some(full) = &self.full
            && !self.rows.iter().any(|row| row.key == full.key)
        {
            self.full = None;
            self.say(format!(
                "{} left the board",
                viewed.unwrap_or_else(|| "the pane".to_owned())
            ));
        }
        self.keep_selection_visible();
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// A jump finished: say where it went, or keep why it failed in view.
    /// A jump, a back or a close came back.
    pub fn went(&mut self, result: Result<String, String>) {
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

    /// New fields and runs from serve. A view or sort on a field that is gone goes back to the
    /// default.
    pub fn set_extensions(&mut self, fields: Vec<Field>, runs: BTreeMap<String, ExtRun>) {
        self.ext_runs = runs;
        if self.fields.iter().map(|(f, _)| f).eq(&fields) {
            return;
        }
        let mut taken = Vec::new();
        self.fields = fields
            .into_iter()
            .map(|field| {
                let letter = free_letter(&field.name, &taken);
                taken.extend(letter);
                (field, letter)
            })
            .collect();
        if !self.views().contains(&self.view) {
            self.view = View::default();
        }
        if let Some((Col::Field(id), _)) = self.sort
            && !self.fields.iter().any(|(f, _)| f.id == id)
        {
            self.sort = None;
        }
    }

    /// The columns of the fields, but for the one the view groups by, whose group header says it.
    pub fn field_columns(&self) -> Vec<&'static Column> {
        self.fields
            .iter()
            .map(|(f, letter)| field_column(f, *letter))
            .filter(|c| {
                self.view
                    .group_by
                    .is_none_or(|key| key != Key::Field(id_of(c)))
            })
            .collect()
    }

    /// The views `v` cycles through: the built-in ones, then one per field, by its name, or by
    /// its id when a built-in view has that name.
    pub fn views(&self) -> Vec<View> {
        let mut all = views();
        for (field, letter) in &self.fields {
            let name = if all.iter().any(|v| v.name == field.name) {
                field.id.clone()
            } else {
                field.name.clone()
            };
            let id = id_of(field_column(field, *letter));
            all.push(View {
                name,
                group_by: Some(Key::Field(id)),
            });
        }
        all
    }

    /// How often the board must redraw for the glyphs on screen to move: the full view's row, or
    /// the list's visible rows and the card's; None when nothing on screen animates.
    pub fn frame_every(&self) -> Option<Duration> {
        if let Some(full) = &self.full {
            let row = self.rows.iter().find(|row| row.key == full.key);
            return super::theme::redraw_every(row.map(super::theme::look), super::theme::motion());
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
        super::theme::redraw_every(
            listed.chain(carded).map(super::theme::look),
            super::theme::motion(),
        )
    }

    /// The column a sort letter names: a built-in one, or a field's.
    fn column_by_letter(&self, letter: char) -> Option<&'static Column> {
        by_letter(letter).or_else(|| {
            let (field, _) = self.fields.iter().find(|(_, l)| *l == Some(letter))?;
            Some(field_column(field, Some(letter)))
        })
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
            .iter()
            .filter_map(|item| self.spot(item))
            .collect()
    }

    /// The spot an item is, if the selection can rest on it: a row, or a folded header.
    fn spot(&self, item: &Item) -> Option<Spot> {
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
        let names: Vec<String> = self
            .views()
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

    /// The `:` prompt's Enter: `seen` marks every done row shown seen, `q` quits, else a view's
    /// name, or the start of exactly one.
    fn open(&mut self) -> Option<Effect> {
        match self.command.value().trim() {
            "seen" => return self.seen_all(),
            "q" | "q!" | "quit" => return Some(Effect::Quit),
            _ => {}
        }
        let typed = match self.command.value().trim() {
            // `flat` was the table view's name before v0.3.
            "flat" => "table".to_owned(),
            typed => typed.to_owned(),
        };
        let all = self.views();
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
        None
    }

    /// `o` and a column's letter: sort by it, flip it if it is the sort, clear it the third time.
    fn sort_by(&mut self, letter: char) {
        let Some(column) = self.column_by_letter(letter) else {
            return;
        };
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

    /// Whether the card shows under the list: unless `c` hid it, and always while it holds the
    /// key table or a failed jump's reason, which only it shows whole. `pick` has none.
    pub fn card_shown(&self) -> bool {
        !self.pick && (!self.hidden || self.keys || self.failed.is_some() || self.answer.is_some())
    }

    fn prefs(&self) -> super::prefs::Prefs {
        super::prefs::Prefs {
            detail_hidden: self.hidden,
            motion: self.motion,
            icons: self.icons,
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

    /// Asks for the current row's tail; the card shows it when it comes.
    fn tail(&mut self) -> Option<Effect> {
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

    /// `space`: the full view of the current row, with its first read.
    fn open_full(&mut self, now: Instant) -> Option<Effect> {
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
    fn full_page(&self) -> (Vec<String>, usize) {
        let lines = self
            .full
            .as_ref()
            .map(|full| super::full::lines(full, self.peek.as_ref()).to_vec())
            .unwrap_or_default();
        (lines, usize::from(self.height.saturating_sub(2)).max(1))
    }

    fn full_scroll(&mut self, by: isize) {
        let (lines, page) = self.full_page();
        if let Some(full) = self.full.as_mut() {
            full.scroll(by, &lines, page);
        }
    }

    /// Keys while the full view is open: it scrolls, answers, jumps or goes back; the board's
    /// own keys wait.
    fn full_key(&mut self, key: K, now: Instant) -> Option<Effect> {
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

    /// `enter`: go to the current row's pane, one jump at a time.
    fn jump(&mut self) -> Option<Effect> {
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
    fn busy(&mut self) -> bool {
        if self.going.is_some() {
            self.say("a jump or close is already running");
        }
        self.going.is_some()
    }

    /// The board's own pane, which it never answers or closes.
    fn is_own(&self, row: &Row) -> bool {
        row.pane.host == "local" && self.own.as_deref() == Some(row.pane.block_id.as_str())
    }

    /// `-`: back to the pane before the last jump, as `cd -` does.
    fn back(&mut self) -> Option<Effect> {
        if self.busy() {
            return None;
        }
        self.going = Some("going back…".into());
        Some(Effect::Back)
    }

    /// `dd`: ask before closing the current row's pane. Never the board's own.
    fn ask_close(&mut self, now: Instant) {
        let Some(row) = self.current() else {
            return;
        };
        if self.is_own(row) || row.pane.process.as_deref() == Some("rexboard") {
            self.failed = Some("that pane runs the board; it stays open".into());
            return;
        }
        self.closing = Some((row.clone(), now));
    }

    /// Where `row` is, as the list tells it apart from the rows that read the same (`#N`), in
    /// `max` columns: the path gives way, never the suffix.
    fn place(&self, row: &Row, max: usize) -> String {
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
        let state = match (row.pane.state.as_str(), row.pane.kind.as_deref()) {
            ("blocked", Some(kind)) if !kind.is_empty() => format!(" (blocked · {kind})"),
            (state @ ("blocked" | "working"), _) => format!(" ({state})"),
            _ => String::new(),
        };
        let tmux = if row.pane.process.as_deref() == Some("tmux") {
            " (tmux keeps running)"
        } else {
            ""
        };
        let full = self.place(row, usize::MAX);
        let ending = format!(", ending {}{state}", harness(row));
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
    fn answer_close(&mut self, row: Row, asked: Instant, key: K, now: Instant) -> Option<Effect> {
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
    fn copy(&mut self, location: bool) -> Option<Effect> {
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

    /// `n` (or `tab`, or `N` the other way): a done row read for READ is seen; then on to the
    /// next NEEDS YOU row, wrapping.
    fn next_waiting(&mut self, now: Instant, back: bool) -> Option<Effect> {
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

    /// A click at `x`, `y` in the table view: on a column's header cell it sorts as that
    /// column's letter does, on its arrow it flips the sort, and on a row it selects the row.
    pub fn click(&mut self, x: u16, y: u16) {
        use super::view::{HEADED, HEADER_ROW};
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

    /// Whether a prompt (`/`, `:` or `f`) has the keyboard, so the mouse leaves the list alone.
    fn prompting(&self) -> bool {
        self.filtering || self.commanding || self.pending == Some('f') || self.answer.is_some()
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

    pub fn handle(&mut self, key: KeyEvent) -> Option<Effect> {
        self.handle_at(key, Instant::now())
    }

    /// One key, at `now`. The layers take it in turn: `dd`'s question, answer mode, the full
    /// view, the `:` and `/` prompts, a pending prefix, then the board's table.
    pub fn handle_at(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        self.failed = None;
        self.quiet = self
            .last_key
            .is_none_or(|at| now.duration_since(at) >= GUARD);
        self.last_key = Some(now);
        let k = K::of(key);
        if let Some((row, asked)) = self.closing.take() {
            return self.answer_close(row, asked, k?, now);
        }
        if self.answer.is_some() {
            return self.answer_key(key, now);
        }
        if self.full.is_some() {
            return self.full_key(k?, now);
        }
        if self.commanding {
            match key.code {
                KeyCode::Esc => self.commanding = false,
                KeyCode::Tab => self.complete(),
                KeyCode::Enter => {
                    self.commanding = false;
                    return self.open();
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
        let k = k?;
        // A ctrl chord after a prefix keeps its own meaning, so ctrl-c still quits.
        if let Some(prefix) = self.pending.take()
            && !k.1
        {
            return self.after(prefix, k, now);
        }
        if keys::starts(keys::BOARD, k) {
            self.pending = k.char();
            self.pending_quiet = self.quiet;
            return None;
        }
        let act = keys::find(keys::BOARD, &[k])?;
        self.act(act, k, now)
    }

    /// The key after a prefix: `o` takes a column's letter, `f` a status's, the rest their
    /// second key from the table. Anything else, esc included, drops the prefix.
    fn after(&mut self, prefix: char, k: K, now: Instant) -> Option<Effect> {
        let letter = k.char();
        match (prefix, letter) {
            ('o', Some(letter)) if self.column_by_letter(letter).is_some() => self.sort_by(letter),
            ('o', _) if k.0 != KeyCode::Esc => self.say(format!(
                "o takes {}; esc cancels",
                self.sort_letters().join(" ")
            )),
            ('f', Some('f')) => self.status = None,
            ('f', Some(letter)) if Status::from_letter(letter).is_some() => {
                self.status = Status::from_letter(letter);
                // Folded groups would hide the very rows asked for.
                self.unfold_all();
            }
            ('f', _) if k.0 != KeyCode::Esc => self.say("f takes w d q p e u i; ff clears"),
            ('o' | 'f', _) => {}
            _ => {
                let act = keys::find(keys::BOARD, &[K(KeyCode::Char(prefix), false), k])?;
                return self.act(act, k, now);
            }
        }
        self.keep_selection_visible();
        None
    }

    /// The sort letters `o` takes, built-in then the fields'.
    pub fn sort_letters(&self) -> Vec<String> {
        super::columns::COLUMNS
            .iter()
            .filter_map(|c| c.letter)
            .chain(self.fields.iter().filter_map(|(_, l)| *l))
            .map(String::from)
            .collect()
    }

    fn act(&mut self, act: Act, k: K, now: Instant) -> Option<Effect> {
        // `pick` only picks: nothing that leaves the list or acts on a pane but going to it.
        if self.pick
            && matches!(
                act,
                Act::Full
                    | Act::NextNeeds
                    | Act::PrevNeeds
                    | Act::Answer
                    | Act::Card
                    | Act::Back
                    | Act::Close
            )
        {
            return None;
        }
        match act {
            Act::Down => self.step(1),
            Act::Up => self.step(-1),
            Act::First => self.selected = self.spots().first().cloned(),
            Act::Last => self.selected = self.spots().last().cloned(),
            Act::Top | Act::Middle | Act::Bottom => {
                if act == Act::Middle {
                    self.tell(
                        'M',
                        "middle of the screen · M used to sort; message is om now",
                    );
                }
                self.on_screen(act);
            }
            Act::HalfDown => self.page(1, 2),
            Act::HalfUp => self.page(-1, 2),
            Act::PageDown => self.page(1, 1),
            Act::PageUp => self.page(-1, 1),
            Act::NextGroup => self.group(false),
            Act::PrevGroup => self.group(true),
            Act::NextNeeds => return self.next_waiting(now, false),
            Act::PrevNeeds => {
                self.tell(
                    'N',
                    "previous row needing you · N used to sort; window is on now",
                );
                return self.next_waiting(now, true);
            }
            Act::Nth => self.nth(k.char()? as usize - '1' as usize),
            Act::Jump => return self.jump(),
            Act::Back => return self.back(),
            Act::Filter => self.filtering = true,
            Act::Status => self.pending = Some('f'),
            Act::Sort => self.pending = Some('o'),
            Act::Command => {
                self.command.reset();
                self.commanding = true;
            }
            Act::NextView | Act::PrevView => {
                let all = self.views();
                let at = all.iter().position(|v| *v == self.view).unwrap_or(0);
                let by = if act == Act::NextView {
                    1
                } else {
                    all.len() - 1
                };
                self.show(all[(at + by) % all.len()].clone());
            }
            Act::FoldToggle | Act::FoldOpen | Act::FoldClose => self.fold(act),
            Act::OpenAll => self.unfold_all(),
            Act::FoldAll => self.fold_all(),
            Act::Card => {
                self.hidden = !self.hidden;
                if self.hidden {
                    self.say(HIDDEN);
                } else {
                    self.toast = None;
                }
                return Some(Effect::SavePrefs(self.prefs()));
            }
            Act::Full => return self.open_full(now),
            Act::Answer => return self.start_answer(now),
            Act::Seen => {
                return self.current().map(|row| Effect::Seen {
                    key: row.key.clone(),
                    label: path(&labels(row, true), usize::MAX),
                    state: row.pane.state.clone(),
                });
            }
            Act::Close if !self.pending_quiet => self.say(TOO_FAST_TO_CLOSE),
            Act::Close => self.ask_close(now),
            Act::CopyMsg => return self.copy(false),
            Act::CopyPlace => return self.copy(true),
            Act::Reconnect => {
                self.tell('R', "reconnecting · R used to sort; harness is oh now");
                self.link = Link::Connecting;
                return Some(Effect::Reconnect);
            }
            Act::Keys => self.keys = !self.keys,
            Act::Cancel if self.pick => return Some(Effect::Quit),
            Act::Cancel if self.keys => self.keys = false,
            Act::Cancel => {
                self.filter.reset();
                self.status = None;
                self.peek = None;
                self.sort = None;
                self.keep_selection_visible();
            }
            // `q` closes the layer in front, and quits only from the bare board.
            Act::Quit if self.keys && k.0 == KeyCode::Char('q') => self.keys = false,
            Act::Quit => return Some(Effect::Quit),
            Act::Moved(note) => self.say(note),
            Act::Leave => {}
        }
        None
    }

    /// Says `note` the first time a run presses `key`, whose meaning changed.
    fn tell(&mut self, key: char, note: &str) {
        if self.told.insert(key) {
            self.say(note);
        }
    }

    /// `1`-`9`: the nth NEEDS YOU row, opening its group if it is folded.
    fn nth(&mut self, n: usize) {
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

    /// The spots drawn on screen now, in order.
    fn on_screen_spots(&self) -> Vec<Spot> {
        let (_, offset, room) = self.drawn.get();
        self.items()
            .iter()
            .skip(offset)
            .take(room.max(1))
            .filter_map(|item| self.spot(item))
            .collect()
    }

    /// `H` `M` `L`: the top, middle or bottom of what the screen shows.
    fn on_screen(&mut self, act: Act) {
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
    fn page(&mut self, dir: isize, part: usize) {
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
    fn group(&mut self, back: bool) {
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

    /// `za` toggles the selection's group, `zo` opens a folded one, `zc` folds a row's.
    fn fold(&mut self, act: Act) {
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
    fn fold_all(&mut self) {
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

    /// `a` or `i`: answer mode on the current row, once a fresh read of its pane comes back.
    /// Pressed in the middle of typing, it opens nothing.
    fn start_answer(&mut self, now: Instant) -> Option<Effect> {
        if !self.quiet {
            self.say(TOO_FAST_TO_ANSWER);
            return None;
        }
        let Some(row) = self.current() else {
            self.say("select a row to answer it");
            return None;
        };
        if self.is_own(row) {
            self.say("that is the board's own pane");
            return None;
        }
        let Some(updated_at) = row.pane.updated_at.clone() else {
            self.say("this pane reports no status, so it cannot be answered here · enter jumps");
            return None;
        };
        let row = row.clone();
        self.full = None;
        self.keys = false;
        let effect = read(&row);
        self.answer = Some(Answer::new(row, updated_at));
        self.answer_guard = Some(now);
        Some(effect)
    }

    /// A key in answer mode. A first key within GUARD of the opening means the opener was
    /// typing meant for somewhere else: answer mode leaves at once, and stays left, so no later
    /// pause lets the rest of the typing pick or fill the reply.
    fn answer_key(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        let leaving = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if let Some(at) = self.answer_guard.take()
            && !leaving
            && now.duration_since(at) < GUARD
        {
            self.answer = None;
            self.say(TOO_FAST_TO_ANSWER);
            return None;
        }
        let answer = self.answer.as_mut()?;
        match answer.key(key, now) {
            Step::Stay => None,
            Step::Leave => {
                self.answer = None;
                None
            }
            Step::Next => {
                self.answer = None;
                self.next_waiting(now, false)
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
                // A tail from before the answer shows the question it answered, and one read now
                // could catch the dialog before the agent takes the key: wait for its next report.
                self.peek.take_if(|peek| peek.key == key);
                if let Some(row) = &latest {
                    self.tailed = Some((row.key.clone(), row.pane.updated_at.clone()));
                }
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

/// A field column's id, as views and sorts name it.
fn id_of(column: &Column) -> &'static str {
    match column.id {
        Col::Field(id) => id,
        _ => unreachable!("a field column"),
    }
}

fn peek(row: &Row, vt: bool) -> Effect {
    Effect::Peek {
        key: row.key.clone(),
        host: row.pane.host.clone(),
        session: row.pane.session_id.clone(),
        block: row.pane.block_id.clone(),
        vt,
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
pub mod testkit;

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::tui::columns::{Col, Dir};

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

    /// Typing meant for an agent that lands on the board: the burst hits `dd` and `a`, and
    /// nothing closes and nothing is sent.
    #[test]
    fn a_typed_burst_closes_nothing_and_sends_nothing() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(0));
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(40));
        assert!(app.close_question(usize::MAX).is_some(), "dd asks");
        assert!(
            app.handle_at(KeyEvent::from(KeyCode::Char('y')), at(80))
                .is_none(),
            "a y within GUARD of the question closes nothing"
        );
        assert_eq!(app.toast(), Some("nothing closed (keys came too fast)"));
        let mut app = super::testkit::app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        let effects = typed_at(&mut app, "added hidden\n", 60, None, IDLE);
        assert!(!effects.iter().any(acts_on_a_pane), "{effects:?}");
        assert!(app.answer.is_none(), "the burst left answer mode");
    }

    /// An idle prompt, as a fast host reads it back.
    const IDLE: &str = "────\n❯\u{a0}\n";

    /// Typing meant for an agent, with one ordinary pause anywhere in it, sends nothing and
    /// picks nothing: an opener only opens on its own, and a fast key after it leaves for good.
    #[test]
    fn typing_with_one_pause_never_sends_or_picks() {
        for text in [
            "please run tests\n",
            "fix it and run tests\n",
            "hidden middle ok\n",
        ] {
            for pause in 0..text.chars().count() {
                let mut app = app();
                let effects = typed_at(&mut app, text, 120, Some(pause), IDLE);
                assert!(
                    !effects.iter().any(acts_on_a_pane),
                    "{text:?} paused at {pause}: {effects:?}"
                );
            }
        }
        for pause in 0.."please run 1 test".len() {
            let mut app = app();
            let effects = typed_at(&mut app, "please run 1 test", 120, Some(pause), DIALOG);
            assert!(
                !effects.iter().any(acts_on_a_pane),
                "paused at {pause}: {effects:?}"
            );
        }
    }

    /// A seeded spread of random typing, 20 to 280 ms a key with one 400 ms pause, at dialogs
    /// and idle prompts: nothing sends, picks or closes.
    #[test]
    fn random_typing_with_a_pause_never_acts_on_a_pane() {
        let mut seed: u64 = 0x5eed;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let letters: Vec<char> = "abcdefghijklmnopqrstuvwxyz123".chars().collect();
        for round in 0..20_000 {
            let mut text = String::new();
            for _ in 0..=next(12) {
                for _ in 0..=next(8) {
                    text.push(letters[next(letters.len() as u64) as usize]);
                }
                text.push(if next(6) == 0 { '\n' } else { ' ' });
            }
            let pause = next(text.chars().count() as u64) as usize;
            let screen = if round % 2 == 0 { IDLE } else { DIALOG };
            let mut app = app();
            let t0 = Instant::now();
            let mut ms = 0;
            for (i, c) in text.chars().enumerate() {
                ms += if i == pause { 400 } else { 20 + next(261) };
                let code = if c == '\n' {
                    KeyCode::Enter
                } else {
                    KeyCode::Char(c)
                };
                let effect = app.handle_at(KeyEvent::from(code), t0 + Duration::from_millis(ms));
                if let Some(Effect::Read { key, .. }) = &effect {
                    app.screen(key, Ok(Screen::parse(screen, None)));
                    drawn(&app);
                }
                // The one way through: the typing starts with `a`, `i` or `dd` and pauses
                // right after it, which is what a person pressing it does.
                let opener_then_pause = (text.starts_with(['a', 'i']) && pause == 1)
                    || (text.starts_with("dd") && pause == 2);
                assert!(
                    !effect.as_ref().is_some_and(acts_on_a_pane) || opener_then_pause,
                    "{text:?} paused at {pause}: {effect:?}"
                );
            }
        }
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

    /// Every key in the board's table does something on a board with rows: an effect, a
    /// change on screen or a word in the footer. None is listed and silently ignored.
    #[test]
    fn every_key_in_the_table_does_something() {
        for entry in keys::BOARD {
            for seq in entry.seqs {
                let did = ["local/seat-a", "mini/seat-b"].iter().any(|from| {
                    let mut app = app();
                    app.drawn.set((3, 0, 20));
                    app.selected = Some(Spot::Row((*from).into()));
                    if matches!(entry.act, Act::Cancel | Act::FoldOpen | Act::OpenAll) {
                        app.status = Some(Status::Done);
                        app.selected = Some(Spot::Group(Bucket::Status(Group::NeedsYou)));
                        app.folded.insert(Bucket::Status(Group::NeedsYou));
                    }
                    let before = state_of(&app);
                    let mut effect = None;
                    for k in seq.iter() {
                        let modifiers = if k.1 {
                            KeyModifiers::CONTROL
                        } else {
                            KeyModifiers::NONE
                        };
                        effect = key(&mut app, KeyEvent::new(k.0, modifiers));
                    }
                    effect.is_some() || state_of(&app) != before
                });
                assert!(did, "{:?} ({seq:?}) did nothing", entry.act);
            }
        }
    }

    #[test]
    fn q_closes_the_layer_in_front_and_quits_only_from_the_bare_board() {
        let mut app = app();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(press(&mut app, KeyCode::Char('q')), None);
        assert!(!app.keys, "q closes the key table");
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(press(&mut app, KeyCode::Char('q')), None);
        assert_eq!(app.filter.value(), "q", "q is text in a prompt");
        press(&mut app, KeyCode::Esc);
        typed(&mut app, ":q");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Quit),
            ":q quits"
        );
        assert_eq!(press(&mut app, KeyCode::Char('q')), Some(Effect::Quit));
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
    fn answer_mode_leaves_on_a_fast_key_and_an_opener_needs_a_quiet_gap() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut app = app();
        app.handle_at(KeyEvent::from(KeyCode::Char('i')), at(0));
        assert!(app.answer.is_some(), "i opens answer mode too");
        app.handle_at(KeyEvent::from(KeyCode::Char('x')), at(200));
        assert!(app.answer.is_none(), "a fast first key leaves");
        assert_eq!(
            app.toast(),
            Some("keys came too fast; nothing sent · a answers")
        );
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(400));
        assert!(
            app.answer.is_none(),
            "an opener right after a key opens nothing"
        );
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(1_000));
        assert!(app.answer.is_some(), "on its own it opens");
        app.handle_at(KeyEvent::from(KeyCode::Esc), at(1_100));
        assert!(app.answer.is_none(), "esc always leaves");
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(2_000));
        app.handle_at(KeyEvent::from(KeyCode::Char('z')), at(2_400));
        assert!(
            app.answer.is_some(),
            "a first key after GUARD is the answer"
        );
        assert_eq!(
            app.answer.as_ref().unwrap().input.value(),
            "",
            "before the read, it waits"
        );
    }

    /// Each old key says where it went and does nothing else; R, M and N, whose meaning
    /// changed, act and say so once a run.
    #[test]
    fn old_keys_say_where_they_went() {
        let mut app = app();
        app.selected = Some(Spot::Row("local/claude".into()));
        for (code, said) in [
            (KeyCode::Char('s'), "mark seen is m now"),
            (KeyCode::Char('S'), "sort by state is os now"),
            (KeyCode::Char('r'), "reconnect is R now"),
            (KeyCode::Char('Y'), "copy the place is yp now"),
        ] {
            assert!(press(&mut app, code).is_none(), "{code:?}");
            assert_eq!(app.toast(), Some(said));
        }
        assert_eq!(app.sort, None, "S no longer sorts");
        for (c, said) in [
            ('s', "mark every done row seen is :seen now"),
            ('k', "close is dd now"),
        ] {
            assert!(key(&mut app, ctrl(c)).is_none());
            assert_eq!(app.toast(), Some(said));
        }
        assert!(matches!(
            press(&mut app, KeyCode::Char('R')),
            Some(Effect::Reconnect)
        ));
        assert_eq!(
            app.toast(),
            Some("reconnecting · R used to sort; harness is oh now")
        );
        app.toast = None;
        press(&mut app, KeyCode::Char('R'));
        assert_eq!(app.toast(), None, "said once a run");
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

    #[test]
    fn full_view_says_when_its_row_leaves_the_board() {
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        press(&mut app, KeyCode::Char(' '));
        app.set_rows(rows().split_off(1), hosts());
        assert!(app.full.is_none());
        assert_eq!(app.toast(), Some("seat-b left the board"));
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
    fn colon_seen_marks_only_unseen_done_rows_and_o_s_sorts() {
        let mut app = App::default();
        let mut all = rows();
        all.push(row("mini/old-done", "done", "needs_you", false));
        all.push(row("mini/seen-done", "done", "quiet", true));
        all.push(row("mini/broken", "error", "needs_you", false));
        app.set_rows(all, hosts());
        assert_eq!(typed(&mut app, "os").pop().flatten(), None, "o s sorts");
        assert_eq!(app.sort, Some((Col::State, Dir::Up)));
        assert_eq!(
            seen_all(&mut app),
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
            seen_all(&mut app),
            Some(Effect::SeenAll(vec!["mini/old-done".into()])),
            "a kept filter narrows it to the rows shown"
        );
        press(&mut app, KeyCode::Esc);
        app.set_rows(
            vec![row("mini/seat-b", "blocked", "needs_you", false)],
            hosts(),
        );
        assert_eq!(seen_all(&mut app), None);
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
    fn m_sends_seen_for_the_selected_key() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(
            press(&mut app, KeyCode::Char('m')),
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

    const DIALOG: &str = " ❯ 1. Yes\n   2. Yes, and don't ask again\n   3. No\n Esc to cancel\n";

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
        assert_eq!(
            (expect.state.as_str(), expect.kind.as_deref()),
            ("blocked", None)
        );
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
        assert!(!app.filtering && app.pending.is_none() && !app.commanding);
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
    fn answer_a_send_holds_the_auto_tail_until_the_next_report() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        // A hook's second report for the same dialog lands while the answer goes out.
        let mut rows = rows();
        rows[0].pane.updated_at = Some("2026-01-01T00:00:06Z".into());
        app.set_rows(rows.clone(), hosts());
        app.sent(&key, "1 (Yes)", Ok(()));
        let t0 = Instant::now();
        app.note_card(t0);
        assert_eq!(
            app.auto_tail(t0 + Duration::from_secs(1)),
            None,
            "not the answered report"
        );
        rows[0].pane.updated_at = Some("2026-01-01T00:01:00Z".into());
        app.set_rows(rows, hosts());
        assert!(
            matches!(
                app.auto_tail(t0 + Duration::from_secs(2)),
                Some(Effect::Peek { .. })
            ),
            "a new ask is read"
        );
    }

    #[test]
    fn a_new_row_in_needs_you_unfolds_it() {
        let mut app = app();
        typed(&mut app, "za");
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
