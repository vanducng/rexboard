use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use unicode_width::UnicodeWidthStr;

use super::answer::{Answer, Step};
use super::columns::{Col, Column, Columns, Sort, UsageShown, field_column, free_letter};
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

mod answer;
mod card;
mod filter;
mod full;
mod pane_keys;
pub(crate) use pane_keys::{RUNS_THE_BOARD, ending, is_own, runs_rexboard, tmux_note};
mod select;
mod views;

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
    /// The blank row above a group's header, never selected.
    Gap,
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

/// The card's last draw: its rows on screen while its text can scroll, how far it is scrolled,
/// and the most it can be.
#[derive(Default, Clone, Copy)]
pub struct CardDrawn {
    pub top: u16,
    pub bottom: u16,
    pub at: usize,
    pub last: usize,
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
    /// ctrl-e and ctrl-y: the row whose card text is scrolled, and how far. Another row starts
    /// where its card opens.
    pub card_scroll: Option<(String, usize)>,
    /// Where the card was last drawn, for the keys and the wheel that scroll it.
    pub card_drawn: std::cell::Cell<CardDrawn>,
    /// Where the list's rows were last drawn: the first row's line, the first item shown, and
    /// how many rows fit. The draw sets it, so a click can find the row under it.
    pub drawn: std::cell::Cell<(u16, usize, usize)>,
    /// Whether the list was drawn with blank rows between its groups, which it is while they fit.
    pub spaced: std::cell::Cell<bool>,
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
    /// tui.json's card height, written back the same way; None is the default.
    pub card_height: Option<super::prefs::CardHeight>,
    /// tui.json's opt-in columns, written back the same way.
    pub columns: Vec<String>,
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
            Act::CardDown => self.card_scroll(1),
            Act::CardUp => self.card_scroll(-1),
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
