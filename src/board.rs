use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::collector::HostEvent;
use crate::error::{Error, Result};
use crate::event::{Event, LabelKind};
use crate::extension::contract::Item;
use crate::extension::field_id;
use crate::pane::{self, Pane, SessionSnap, Source};

pub type RowKey = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    NeedsYou,
    Working,
    Quiet,
    Unreported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// `host/block_id`, the key `seen` and `next` take.
    pub key: String,
    #[serde(flatten)]
    pub pane: Pane,
    pub since: String,
    pub since_estimated: bool,
    pub turn_started: Option<String>,
    /// Seconds from entering `working` to the next `done` or `blocked`.
    pub last_turn: Option<i64>,
    pub seen: bool,
    pub stale: bool,
    /// The record says `working`, but it is old and the pane looks idle, so it is not trusted.
    #[serde(default)]
    pub suspect: bool,
    #[serde(default)]
    pub suspect_reason: Option<String>,
    pub group: Group,
    /// Extension fields, by id (`<extension>.<field>`).
    #[serde(default)]
    pub ext: BTreeMap<String, String>,
    /// What the agent's last turn used, for a pane on serve's own host; see `rexboard usage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::harness::usage::Usage>,
}

/// How old a `working` report can get before a pane that looks idle makes it suspect.
const SUSPECT_AFTER_SECS: i64 = 120;

const SHELLS: [&str; 4] = ["zsh", "bash", "fish", "sh"];

#[derive(Debug)]
struct Entry {
    src: Source,
    pane: Pane,
    since: DateTime<Utc>,
    since_estimated: bool,
    turn_started: Option<DateTime<Utc>>,
    last_turn: Option<i64>,
    seen: bool,
    stale: bool,
    ext: BTreeMap<String, String>,
}

impl Entry {
    /// Why a `working` record is not believed: its last report is old and the pane disagrees,
    /// as when a hook never reported the turn's end.
    fn suspect(&self, now: DateTime<Utc>) -> Option<String> {
        let p = &self.pane;
        if p.state != "working" || p.source == "inferred" {
            return None;
        }
        let at = DateTime::parse_from_rfc3339(p.updated_at.as_deref()?).ok()?;
        let age = now - at.with_timezone(&Utc);
        if age.num_seconds() <= SUSPECT_AFTER_SECS {
            return None;
        }
        let at = at
            .with_timezone(&chrono::Local)
            .format(if age.num_days() >= 1 {
                "%b %-d %H:%M"
            } else {
                "%H:%M"
            });
        // Under tmux the title is tmux's and keeps its idle glyph while the agent works.
        let process = self.src.process.as_deref();
        if process != Some("tmux") && p.title.starts_with("✳ ") {
            return Some(format!("reported working at {at}; the title shows idle"));
        }
        let shell = process.filter(|name| SHELLS.contains(name))?;
        Some(format!(
            "reported working at {at}; {shell} is back in front"
        ))
    }

    fn group(&self, now: DateTime<Utc>) -> Group {
        let p = &self.pane;
        if self.stale
            || p.source == "inferred"
            || p.state == "unknown"
            || self.suspect(now).is_some()
        {
            return Group::Unreported;
        }
        match p.state.as_str() {
            "blocked" | "error" => Group::NeedsYou,
            "done" if !self.seen => Group::NeedsYou,
            "working" => Group::Working,
            _ => Group::Quiet,
        }
    }

    fn row(&self, now: DateTime<Utc>) -> Row {
        let suspect_reason = self.suspect(now);
        Row {
            key: format!("{}/{}", self.pane.host, self.pane.block_id),
            pane: self.pane.clone(),
            since: stamp(self.since),
            since_estimated: self.since_estimated,
            turn_started: self.turn_started.map(stamp),
            last_turn: self.last_turn,
            seen: self.seen,
            stale: self.stale,
            suspect: suspect_reason.is_some(),
            suspect_reason,
            group: self.group(now),
            ext: self.ext.clone(),
            usage: None,
        }
    }
}

/// Pane rows for every host, with the history Rex does not keep: since, turn and seen.
#[derive(Debug, Default)]
pub struct Board {
    rows: BTreeMap<RowKey, Entry>,
    sessions: BTreeMap<(String, String), String>,
    tabs: BTreeMap<(String, String), String>,
    versions: BTreeMap<String, String>,
    warned: BTreeSet<String>,
    warnings: Vec<String>,
    transitions: Vec<Transition>,
}

/// What a restart keeps for a pane, in `state.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub state: String,
    pub kind: Option<String>,
    pub since: String,
    pub since_estimated: bool,
    pub turn_started: Option<String>,
    pub last_turn: Option<i64>,
    pub seen: bool,
}

/// One state change, appended to `transitions.jsonl`.
#[derive(Debug, Clone, Serialize)]
pub struct Transition {
    pub at: String,
    pub host: String,
    pub block_id: String,
    pub from: String,
    pub to: String,
    pub kind: Option<String>,
}

impl Board {
    /// Applies one host event at `now` and returns the rows it changed.
    pub fn apply(
        &mut self,
        host: &str,
        event: &HostEvent,
        now: DateTime<Utc>,
    ) -> Result<Vec<RowKey>> {
        let event = match event {
            HostEvent::Event(event, _) => event,
            HostEvent::Down { .. } => return Ok(self.mark_stale(host)),
            HostEvent::Failed(_) => return Ok(Vec::new()),
        };
        match event {
            Event::Snapshot {
                sessions,
                rex_version,
            } => {
                if let Some(version) = rex_version {
                    self.versions.insert(host.to_owned(), version.clone());
                }
                self.snapshot(host, sessions, now)
            }
            Event::SessionCreated { session } => {
                let sessions = self.parse(
                    host,
                    pane::parse_sessions(&Value::Array(vec![session.clone()])),
                )?;
                let mut keys = Vec::new();
                for session in sessions {
                    keys.extend(self.add_session(host, session, now, false)?);
                }
                Ok(keys)
            }
            Event::Status {
                block_id,
                record_id,
                record,
                ..
            } => {
                let record = self.parse(host, pane::parse_record(record))?;
                if let Some(at) = &record.updated_at {
                    self.time(host, at);
                }
                let id = record_id.clone().unwrap_or_default();
                Ok(self.change(host, block_id, now, |src| {
                    src.records.insert(id, record);
                }))
            }
            Event::StatusRemoved { block_id, ids, .. } => {
                Ok(self.change(host, block_id, now, |src| {
                    if ids.is_empty() {
                        src.records.clear();
                    }
                    for id in ids {
                        src.records.remove(id);
                    }
                }))
            }
            Event::Title {
                block_id, title, ..
            } => Ok(self.change(host, block_id, now, |src| {
                src.title = title.clone().unwrap_or_default();
            })),
            Event::Process { block_id, name, .. } => Ok(self.change(host, block_id, now, |src| {
                src.process.clone_from(name);
            })),
            Event::BlockCreated {
                session_id,
                window_id,
                block,
            } => {
                let src = Source::new(
                    session_id,
                    window_id,
                    self.parse(host, pane::parse_block(block))?,
                );
                Ok(vec![self.insert(host, src, now, false)?])
            }
            Event::BlockClosed { block_id, .. } => {
                let key = (host.to_owned(), block_id.clone());
                Ok(self.rows.remove(&key).map(|_| key).into_iter().collect())
            }
            Event::WindowCreated {
                window_id, label, ..
            } => {
                let tab = (host.to_owned(), window_id.clone());
                self.tabs.insert(tab, label.clone().unwrap_or_default());
                Ok(Vec::new())
            }
            Event::WindowClosed { window_id, .. } => {
                self.tabs.remove(&(host.to_owned(), window_id.clone()));
                Ok(self.remove(host, |src| &src.window_id == window_id))
            }
            Event::SessionDestroyed { session_id } => {
                self.sessions.remove(&(host.to_owned(), session_id.clone()));
                Ok(self.remove(host, |src| &src.session_id == session_id))
            }
            Event::Label {
                kind, id, label, ..
            } => {
                let label = label.clone().unwrap_or_default();
                match kind {
                    LabelKind::Session => {
                        self.sessions.insert((host.to_owned(), id.clone()), label);
                        Ok(self.relabel(host, |src| &src.session_id == id))
                    }
                    LabelKind::Window => {
                        self.tabs.insert((host.to_owned(), id.clone()), label);
                        Ok(self.relabel(host, |src| &src.window_id == id))
                    }
                    LabelKind::Block => Ok(self.change(host, id, now, |src| src.label = label)),
                }
            }
            Event::ChildExited { .. }
            | Event::ActiveWindow { .. }
            | Event::Tick
            | Event::Unknown => Ok(Vec::new()),
        }
    }

    /// Rows in board order at `now`: by group, then longest in state first.
    pub fn rows(&self, now: DateTime<Utc>) -> Vec<Row> {
        let mut entries: Vec<&Entry> = self.rows.values().collect();
        entries.sort_by_key(|e| (e.group(now), urgency(e, now), e.since));
        entries.into_iter().map(|entry| entry.row(now)).collect()
    }

    /// The rows that are suspect at `now`. Time alone can make a row suspect, so serve checks
    /// this every second, not only on events.
    pub fn suspects(&self, now: DateTime<Utc>) -> BTreeSet<RowKey> {
        self.rows
            .iter()
            .filter(|(_, entry)| entry.suspect(now).is_some())
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn snapshot(
        &mut self,
        host: &str,
        sessions: &Value,
        now: DateTime<Utc>,
    ) -> Result<Vec<RowKey>> {
        let sessions = self.parse(host, pane::parse_sessions(sessions))?;
        let old: BTreeMap<RowKey, Entry> = self
            .keys(host, |_| true)
            .into_iter()
            .filter_map(|key| self.rows.remove_entry(&key))
            .collect();
        self.sessions.retain(|(h, _), _| h != host);
        self.tabs.retain(|(h, _), _| h != host);
        let mut keys: Vec<RowKey> = old.keys().cloned().collect();
        for session in sessions {
            keys.extend(self.add_session(host, session, now, true)?);
        }
        // A snapshot knows nothing of extensions, so their fields and status carry over until
        // their next run.
        for (key, prev) in &old {
            let Some(entry) = self.rows.get_mut(key) else {
                continue;
            };
            entry.ext.clone_from(&prev.ext);
            if prev.src.fallback.is_some() {
                entry.src.fallback.clone_from(&prev.src.fallback);
                let pane = self.derive(host, &self.rows[key].src);
                self.rows.get_mut(key).expect("listed row").pane = pane;
            }
        }
        for (key, prev) in old {
            if let Some(entry) = self.rows.get_mut(&key)
                && (&entry.pane.state, &entry.pane.kind) == (&prev.pane.state, &prev.pane.kind)
            {
                entry.since = prev.since;
                entry.since_estimated = prev.since_estimated;
                entry.turn_started = prev.turn_started;
                entry.last_turn = prev.last_turn;
                entry.seen = prev.seen;
            }
        }
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    fn add_session(
        &mut self,
        host: &str,
        session: SessionSnap,
        now: DateTime<Utc>,
        seeded: bool,
    ) -> Result<Vec<RowKey>> {
        self.sessions
            .insert((host.to_owned(), session.session_id.clone()), session.label);
        let mut keys = Vec::new();
        for window in session.windows {
            self.tabs
                .insert((host.to_owned(), window.window_id.clone()), window.label);
            for block in window.blocks {
                let src = Source::new(&session.session_id, &window.window_id, block);
                keys.push(self.insert(host, src, now, seeded)?);
            }
        }
        Ok(keys)
    }

    /// A row we did not see change. `seeded` means it came from a snapshot, so `since` is a guess.
    fn insert(
        &mut self,
        host: &str,
        src: Source,
        now: DateTime<Utc>,
        seeded: bool,
    ) -> Result<RowKey> {
        for record in src.records.values() {
            if let Some(at) = &record.updated_at {
                self.time(host, at);
            }
        }
        let pane = self.derive(host, &src);
        let reported = pane
            .updated_at
            .as_deref()
            .and_then(|at| self.time(host, at));
        let unreadable = pane.updated_at.is_some() && reported.is_none();
        let since = reported.unwrap_or(now);
        let entry = Entry {
            since,
            since_estimated: unreadable
                || seeded && (reported.is_none() || pane.state == "working"),
            turn_started: (pane.state == "working").then_some(since),
            last_turn: None,
            seen: !matches!(pane.state.as_str(), "done" | "blocked" | "error"),
            stale: false,
            ext: BTreeMap::new(),
            src,
            pane,
        };
        let key = (host.to_owned(), entry.src.block_id.clone());
        self.rows.insert(key.clone(), entry);
        Ok(key)
    }

    fn change(
        &mut self,
        host: &str,
        block_id: &str,
        now: DateTime<Utc>,
        edit: impl FnOnce(&mut Source),
    ) -> Vec<RowKey> {
        let key = (host.to_owned(), block_id.to_owned());
        let Some(mut entry) = self.rows.remove(&key) else {
            return Vec::new();
        };
        edit(&mut entry.src);
        let pane = self.derive(host, &entry.src);
        if (&pane.state, &pane.kind) != (&entry.pane.state, &entry.pane.kind) {
            self.transitions.push(Transition {
                at: stamp(now),
                host: host.to_owned(),
                block_id: block_id.to_owned(),
                from: entry.pane.state.clone(),
                to: pane.state.clone(),
                kind: pane.kind.clone(),
            });
            let was_working = entry.pane.state == "working";
            entry.since = now;
            entry.since_estimated = false;
            match pane.state.as_str() {
                "working" => {
                    entry.turn_started = Some(now);
                    entry.seen = true;
                }
                "done" | "blocked" | "error" => {
                    if was_working && pane.state != "error" {
                        entry.last_turn = entry.turn_started.map(|t| (now - t).num_seconds());
                    }
                    entry.seen = false;
                }
                _ => {}
            }
        }
        entry.pane = pane;
        entry.stale = false;
        self.rows.insert(key.clone(), entry);
        vec![key]
    }

    /// An extension's latest answer: its fields replace the ones it gave before, and its status,
    /// when the manifest provides one, becomes the fallback of a row with no native record.
    /// Another extension's fallback is kept until it lets go. Returns the rows that changed.
    pub fn extension(
        &mut self,
        name: &str,
        results: &BTreeMap<String, Item>,
        provides_status: bool,
        now: DateTime<Utc>,
    ) -> Vec<RowKey> {
        let prefix = format!("{name}.");
        let keys: Vec<RowKey> = self.rows.keys().cloned().collect();
        let mut changed = Vec::new();
        for key in keys {
            let item = results.get(&format!("{}/{}", key.0, key.1));
            let entry = self.rows.get_mut(&key).expect("listed row");
            let mut fields = entry.ext.clone();
            fields.retain(|id, _| !id.starts_with(&prefix));
            for (field, value) in item.iter().flat_map(|item| &item.fields) {
                fields.insert(field_id(name, field), value.clone());
            }
            let fields_changed = fields != entry.ext;
            entry.ext = fields;
            let mine = item
                .and_then(|item| item.status.clone())
                .filter(|_| provides_status)
                .map(|status| (name.to_owned(), status));
            let held_by_other = entry
                .src
                .fallback
                .as_ref()
                .is_some_and(|(holder, _)| holder != name);
            let fallback = if held_by_other {
                entry.src.fallback.clone()
            } else {
                mine
            };
            if fallback != entry.src.fallback {
                changed.extend(self.change(&key.0, &key.1, now, |src| src.fallback = fallback));
            } else if fields_changed {
                changed.push(key);
            }
        }
        changed
    }

    /// Marks every row of `host` stale, so they move to `unreported` without being dropped.
    pub fn mark_stale(&mut self, host: &str) -> Vec<RowKey> {
        let keys = self.keys(host, |_| true);
        for key in &keys {
            self.rows.get_mut(key).expect("listed row").stale = true;
        }
        keys
    }

    /// Marks a row seen: the person acted on it. False if there is no such row.
    pub fn mark_seen(&mut self, key: &RowKey) -> bool {
        self.rows
            .get_mut(key)
            .map(|entry| entry.seen = true)
            .is_some()
    }

    /// Drops a host that left `rex hosts`.
    pub fn forget(&mut self, host: &str) -> Vec<RowKey> {
        self.sessions.retain(|(h, _), _| h != host);
        self.tabs.retain(|(h, _), _| h != host);
        self.versions.remove(host);
        self.remove(host, |_| true)
    }

    /// Every row's memory, keyed `host/block_id`.
    pub fn memory(&self) -> BTreeMap<String, Memory> {
        self.rows
            .values()
            .map(|e| {
                let memory = Memory {
                    state: e.pane.state.clone(),
                    kind: e.pane.kind.clone(),
                    since: stamp(e.since),
                    since_estimated: e.since_estimated,
                    turn_started: e.turn_started.map(stamp),
                    last_turn: e.last_turn,
                    seen: e.seen,
                };
                (format!("{}/{}", e.pane.host, e.pane.block_id), memory)
            })
            .collect()
    }

    /// Puts back what a restart kept for `host`'s panes whose state and kind did not change.
    pub fn restore(&mut self, host: &str, memory: &BTreeMap<String, Memory>) -> Vec<RowKey> {
        let mut keys = Vec::new();
        for (key, entry) in self.rows.iter_mut().filter(|((h, _), _)| h == host) {
            let Some(m) = memory.get(&format!("{host}/{}", key.1)) else {
                continue;
            };
            if (&m.state, &m.kind) != (&entry.pane.state, &entry.pane.kind) {
                continue;
            }
            let Ok(since) = DateTime::parse_from_rfc3339(&m.since) else {
                continue;
            };
            entry.since = since.with_timezone(&Utc);
            entry.since_estimated = m.since_estimated;
            entry.turn_started = m
                .turn_started
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc));
            entry.last_turn = m.last_turn;
            entry.seen = m.seen;
            keys.push(key.clone());
        }
        keys
    }

    pub fn take_transitions(&mut self) -> Vec<Transition> {
        std::mem::take(&mut self.transitions)
    }

    /// Warnings gathered since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    fn version(&self, host: &str) -> &str {
        self.versions
            .get(host)
            .map_or("version unknown", String::as_str)
    }

    fn parse<T>(&self, host: &str, result: serde_json::Result<T>) -> Result<T> {
        result.map_err(|err| {
            Error::Rex(format!(
                "unexpected pane shape from Rex {}: {err}",
                self.version(host)
            ))
        })
    }

    /// An unreadable time counts as missing. One warning per host, so a long serve stays quiet.
    fn time(&mut self, host: &str, at: &str) -> Option<DateTime<Utc>> {
        if let Ok(t) = DateTime::parse_from_rfc3339(at) {
            return Some(t.with_timezone(&Utc));
        }
        if self.warned.insert(host.to_owned()) {
            self.warnings.push(format!(
                "warning: {host}: updated_at {at:?} is not RFC 3339 (Rex {}); time in state is estimated",
                self.version(host)
            ));
        }
        None
    }

    fn relabel(&mut self, host: &str, hit: impl Fn(&Source) -> bool) -> Vec<RowKey> {
        let keys = self.keys(host, hit);
        for key in &keys {
            let pane = self.derive(host, &self.rows[key].src);
            self.rows.get_mut(key).expect("listed row").pane = pane;
        }
        keys
    }

    fn remove(&mut self, host: &str, hit: impl Fn(&Source) -> bool) -> Vec<RowKey> {
        let keys = self.keys(host, hit);
        for key in &keys {
            self.rows.remove(key);
        }
        keys
    }

    fn keys(&self, host: &str, hit: impl Fn(&Source) -> bool) -> Vec<RowKey> {
        self.rows
            .iter()
            .filter(|((h, _), entry)| h == host && hit(&entry.src))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn derive(&self, host: &str, src: &Source) -> Pane {
        let label = |map: &BTreeMap<(String, String), String>, id: &str| {
            map.get(&(host.to_owned(), id.to_owned()))
                .cloned()
                .unwrap_or_default()
        };
        pane::derive(
            host,
            &label(&self.sessions, &src.session_id),
            &label(&self.tabs, &src.window_id),
            src,
        )
    }
}

/// Inside NeedsYou, blocked comes before error, then unseen done.
fn urgency(entry: &Entry, now: DateTime<Utc>) -> u8 {
    match entry.group(now) {
        Group::NeedsYou => pane::rank(&entry.pane.state),
        _ => 0,
    }
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::event;

    const FIXTURE: &str = include_str!("../tests/fixtures/board-turns.ndjson");

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + TimeDelta::seconds(secs)
    }

    fn line(text: &str) -> HostEvent {
        let (event, raw) = event::parse(text, "test").unwrap();
        HostEvent::Event(event, raw)
    }

    /// Applies the first `n` fixture lines, line `i` at second `i`.
    fn replay(n: usize) -> Board {
        let mut board = Board::default();
        for (i, text) in FIXTURE.lines().take(n).enumerate() {
            board.apply("mini", &line(text), at(i as i64)).unwrap();
        }
        board
    }

    fn row(board: &Board, block: &str) -> Row {
        board
            .rows(at(10_000))
            .into_iter()
            .find(|r| r.pane.block_id == block)
            .unwrap()
    }

    fn snapshot(blocks: Value) -> HostEvent {
        line(
            &json!({ "v": 1, "type": "snapshot", "sessions": [{
                "session_id": "session:api", "label": "api",
                "windows": [{ "window_id": "window:api", "label": "main", "blocks": blocks }]
            }]})
            .to_string(),
        )
    }

    fn block(id: &str, state: &str, updated_at: &str) -> Value {
        json!({ "block_id": id, "label": "shell", "title": "", "process": {},
            "records": [{ "state": state, "app": "claude-code", "updated_at": updated_at }] })
    }

    const PANE: &str = "block:web-claude";

    #[test]
    fn since_holds_across_same_state_reports() {
        for n in 6..=9 {
            let row = row(&replay(n), PANE);
            assert_eq!(row.pane.state, "working");
            assert_eq!(row.since, stamp(at(5)), "after line {n}");
            assert!(row.seen && !row.since_estimated);
        }
        assert_eq!(
            row(&replay(9), PANE).pane.msg.as_deref(),
            Some("Still testing")
        );
    }

    #[test]
    fn blocked_after_working_sets_since_and_unseen() {
        let row = row(&replay(10), PANE);
        assert_eq!(row.pane.state, "blocked");
        assert_eq!(row.pane.kind.as_deref(), Some("permission"));
        assert_eq!(row.since, stamp(at(9)));
        assert!(!row.seen);
        assert_eq!(row.group, Group::NeedsYou);
        assert_eq!(row.last_turn, Some(4));
    }

    #[test]
    fn done_after_working_records_last_turn() {
        let row = row(&replay(13), PANE);
        assert_eq!(
            row.pane.state, "done",
            "a subagent record must not flip the row"
        );
        assert_eq!(
            row.pane.children,
            BTreeMap::from([("working".to_owned(), 1)])
        );
        assert_eq!(row.turn_started, Some(stamp(at(10))));
        assert_eq!(row.last_turn, Some(2));
        assert!(!row.seen);
    }

    #[test]
    fn prompt_removal_falls_back_to_title_inference() {
        let row = row(&replay(15), PANE);
        assert_eq!(row.pane.source, "inferred");
        assert_eq!(row.pane.state, "idle");
        assert_eq!(row.pane.harness.as_deref(), Some("claude-code"));
        assert!(row.pane.children.is_empty());
        assert_eq!(row.group, Group::Unreported);
        let gone = replay(21);
        assert!(
            gone.rows(at(10_000))
                .iter()
                .all(|r| r.pane.block_id != PANE)
        );
    }

    #[test]
    fn reconnect_snapshot_keeps_since_for_unchanged_state() {
        let mut board = Board::default();
        let first = snapshot(json!([
            block("block:a", "working", "2026-01-01T00:00:00Z"),
            block("block:b", "idle", "2026-01-01T00:00:00Z"),
        ]));
        board.apply("mini", &first, at(10)).unwrap();
        let down = HostEvent::Down {
            reason: "gone".into(),
            retry_in: Duration::from_secs(1),
        };
        board.apply("mini", &down, at(20)).unwrap();
        let again = snapshot(json!([
            block("block:a", "working", "2026-01-01T00:00:25Z"),
            block("block:b", "done", "2026-01-01T00:00:25Z"),
        ]));
        board.apply("mini", &again, at(30)).unwrap();
        let a = row(&board, "block:a");
        assert_eq!(a.since, stamp(at(0)));
        assert!(a.since_estimated && !a.stale);
        let b = row(&board, "block:b");
        assert_eq!(b.since, stamp(at(25)));
        assert!(!b.seen);
    }

    #[test]
    fn needs_you_orders_blocked_first_then_longest_waiting() {
        let mut board = Board::default();
        let blocks = json!([
            block("block:done", "done", "2026-01-01T00:00:01Z"),
            block("block:error", "error", "2026-01-01T00:00:02Z"),
            block("block:late", "blocked", "2026-01-01T00:00:09Z"),
            block("block:early", "blocked", "2026-01-01T00:00:05Z"),
            block("block:busy", "working", "2026-01-01T00:00:00Z"),
        ]);
        board.apply("mini", &snapshot(blocks), at(10)).unwrap();
        let order: Vec<_> = board
            .rows(at(10_000))
            .into_iter()
            .map(|r| (r.group, r.pane.block_id))
            .collect();
        assert_eq!(
            order,
            [
                (Group::NeedsYou, "block:early".to_owned()),
                (Group::NeedsYou, "block:late".to_owned()),
                (Group::NeedsYou, "block:error".to_owned()),
                (Group::NeedsYou, "block:done".to_owned()),
                (Group::Working, "block:busy".to_owned()),
            ]
        );
    }

    #[test]
    fn stale_host_rows_move_to_unreported() {
        let mut board = Board::default();
        let blocks = json!([block("block:a", "blocked", "2026-01-01T00:00:00Z")]);
        board
            .apply("mini", &snapshot(blocks.clone()), at(1))
            .unwrap();
        board.apply("studio", &snapshot(blocks), at(1)).unwrap();
        let down = HostEvent::Down {
            reason: "gone".into(),
            retry_in: Duration::from_secs(1),
        };
        assert_eq!(board.apply("mini", &down, at(2)).unwrap().len(), 1);
        let rows = board.rows(at(10_000));
        let mini = rows.iter().find(|r| r.pane.host == "mini").unwrap();
        let studio = rows.iter().find(|r| r.pane.host == "studio").unwrap();
        assert!(mini.stale);
        assert_eq!(mini.group, Group::Unreported);
        assert_eq!(studio.group, Group::NeedsYou);
    }

    #[test]
    fn updated_at_is_parsed_not_compared_as_text() {
        let mut board = Board::default();
        // As text the -03:00 time sorts first; as a time it is an hour later.
        let blocks = json!([
            block("block:minus3", "done", "2026-01-01T02:00:00-03:00"),
            block("block:utc", "done", "2026-01-01T04:00:00Z"),
        ]);
        board.apply("mini", &snapshot(blocks), at(0)).unwrap();
        let rows = board.rows(at(10_000));
        assert_eq!(rows[0].pane.block_id, "block:utc");
        assert_eq!(rows[0].since, "2026-01-01T04:00:00.000Z");
        assert_eq!(rows[1].pane.block_id, "block:minus3");
        assert_eq!(rows[1].since, "2026-01-01T05:00:00.000Z");
    }

    #[test]
    fn unreadable_updated_at_is_estimated_with_one_warning_per_host() {
        let mut board = Board::default();
        let blocks = json!([
            block("block:x", "done", "yesterday"),
            block("block:y", "done", "last week"),
        ]);
        board.apply("mini", &snapshot(blocks), at(7)).unwrap();
        let row = row(&board, "block:x");
        assert_eq!(row.since, stamp(at(7)));
        assert!(row.since_estimated);
        assert_eq!(
            board.take_warnings(),
            [
                r#"warning: mini: updated_at "yesterday" is not RFC 3339 (Rex version unknown); time in state is estimated"#
            ]
        );
        assert!(board.take_warnings().is_empty());
    }

    #[test]
    fn bad_record_names_the_rex_version() {
        let mut board = Board::default();
        let snap = r#"{"v":1,"type":"snapshot","rex_version":"0.0.1-gabc","sessions":{}}"#;
        board.apply("mini", &line(snap), at(0)).unwrap();
        let status = r#"{"v":1,"type":"status","session_id":"s","block_id":"b","record_id":"","record":{"app":"x"}}"#;
        let err = board.apply("mini", &line(status), at(1)).unwrap_err();
        assert_eq!(err.code(), "REX");
        assert!(
            err.to_string()
                .starts_with("unexpected pane shape from Rex 0.0.1-gabc: missing field `state`"),
            "{err}"
        );
    }

    fn working(id: &str, title: &str, process: &str, updated_at: &str) -> Value {
        json!({ "block_id": id, "label": "seat-a", "title": title, "process": { "name": process },
            "records": [{ "state": "working", "app": "claude-code", "updated_at": updated_at,
                          "owner": { "name": "tmux" } }] })
    }

    #[test]
    fn an_old_working_report_with_an_idle_pane_is_suspect() {
        let mut board = Board::default();
        let blocks = json!([
            working(
                "block:idle-title",
                "✳ seat-a@demo",
                "2.1.0",
                "2026-01-01T00:00:00Z"
            ),
            working(
                "block:under-tmux",
                "✳ seat-t@demo",
                "tmux",
                "2026-01-01T00:00:00Z"
            ),
            working("block:shell", "", "zsh", "2026-01-01T00:00:00Z"),
            working(
                "block:busy",
                "◐ seat-a@demo",
                "2.1.0",
                "2026-01-01T00:00:00Z"
            ),
        ]);
        board.apply("mini", &snapshot(blocks), at(5)).unwrap();
        let fresh = board.rows(at(SUSPECT_AFTER_SECS));
        assert!(
            fresh.iter().all(|r| !r.suspect),
            "a recent report is trusted"
        );
        let later = board.rows(at(SUSPECT_AFTER_SECS + 1));
        let get = |id: &str| later.iter().find(|r| r.pane.block_id == id).unwrap();
        let idle = get("block:idle-title");
        assert!(idle.suspect);
        assert_eq!(idle.pane.state, "working", "the raw state stays");
        assert_eq!(idle.group, Group::Unreported);
        assert!(
            idle.suspect_reason
                .as_deref()
                .unwrap()
                .ends_with("; the title shows idle")
        );
        let tmux = get("block:under-tmux");
        assert!(
            !tmux.suspect,
            "under tmux the title is tmux's, not the agent's"
        );
        assert_eq!(tmux.group, Group::Working);
        let shell = get("block:shell");
        assert!(shell.suspect);
        assert!(
            shell
                .suspect_reason
                .as_deref()
                .unwrap()
                .ends_with("; zsh is back in front")
        );
        let busy = get("block:busy");
        assert!(!busy.suspect, "the harness is still in front");
        assert_eq!(busy.group, Group::Working);
        assert_eq!(
            board.suspects(at(SUSPECT_AFTER_SECS + 1)).len(),
            2,
            "serve can see a row turn suspect with time alone"
        );
        let day_old = board.rows(at(86_400 + 5));
        let reason = day_old
            .iter()
            .find(|r| r.pane.block_id == "block:shell")
            .and_then(|r| r.suspect_reason.clone())
            .unwrap();
        assert!(
            reason.starts_with("reported working at Jan 1 ")
                || reason.starts_with("reported working at Dec 31 "),
            "a report a day old gives its date: {reason}"
        );
    }

    #[test]
    fn rows_from_a_serve_without_suspect_fields_parse() {
        let row = serde_json::to_value(replay(3).rows(at(3)).remove(0)).unwrap();
        let mut old = row.as_object().unwrap().clone();
        old.remove("suspect");
        old.remove("suspect_reason");
        let parsed: Row = serde_json::from_value(Value::Object(old)).unwrap();
        assert!(!parsed.suspect && parsed.suspect_reason.is_none());
    }
}
