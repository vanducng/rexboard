use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::collector::HostEvent;
use crate::error::{Error, Result};
use crate::event::{Event, LabelKind};
use crate::pane::{self, Pane, SessionSnap, Source};

pub type RowKey = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    NeedsYou,
    Working,
    Quiet,
    NoSignal,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub pane: Pane,
    pub since: String,
    pub since_estimated: bool,
    pub turn_started: Option<String>,
    /// Seconds from entering `working` to the next `done` or `blocked`.
    pub last_turn: Option<i64>,
    pub seen: bool,
    pub stale: bool,
    pub group: Group,
}

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
}

impl Entry {
    fn group(&self) -> Group {
        let p = &self.pane;
        if self.stale || p.source == "inferred" || p.state == "unknown" {
            return Group::NoSignal;
        }
        match p.state.as_str() {
            "blocked" | "error" => Group::NeedsYou,
            "done" if !self.seen => Group::NeedsYou,
            "working" => Group::Working,
            _ => Group::Quiet,
        }
    }

    fn row(&self) -> Row {
        Row {
            pane: self.pane.clone(),
            since: stamp(self.since),
            since_estimated: self.since_estimated,
            turn_started: self.turn_started.map(stamp),
            last_turn: self.last_turn,
            seen: self.seen,
            stale: self.stale,
            group: self.group(),
        }
    }
}

/// Pane rows for every host, with the history Rex does not keep: since, turn and seen.
#[derive(Debug, Default)]
pub struct Board {
    rows: BTreeMap<RowKey, Entry>,
    sessions: BTreeMap<(String, String), String>,
    tabs: BTreeMap<(String, String), String>,
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
            HostEvent::Down { .. } => {
                let keys = self.keys(host, |_| true);
                for key in &keys {
                    self.rows.get_mut(key).expect("listed row").stale = true;
                }
                return Ok(keys);
            }
            HostEvent::Failed(_) => return Ok(Vec::new()),
        };
        match event {
            Event::Snapshot { sessions, .. } => self.snapshot(host, sessions, now),
            Event::SessionCreated { session } => {
                let sessions = parse(pane::parse_sessions(&Value::Array(vec![session.clone()])))?;
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
                let record = parse(pane::parse_record(record))?;
                if let Some(at) = &record.updated_at {
                    time(at)?;
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
                let src = Source::new(session_id, window_id, parse(pane::parse_block(block))?);
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

    /// Rows in board order: by group, then longest in state first.
    pub fn rows(&self) -> Vec<Row> {
        let mut entries: Vec<&Entry> = self.rows.values().collect();
        entries.sort_by(|a, b| {
            (a.group(), urgency(a), a.since).cmp(&(b.group(), urgency(b), b.since))
        });
        entries.into_iter().map(Entry::row).collect()
    }

    fn snapshot(
        &mut self,
        host: &str,
        sessions: &Value,
        now: DateTime<Utc>,
    ) -> Result<Vec<RowKey>> {
        let sessions = parse(pane::parse_sessions(sessions))?;
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
                time(at)?;
            }
        }
        let pane = self.derive(host, &src);
        let reported = pane.updated_at.as_deref().map(time).transpose()?;
        let since = reported.unwrap_or(now);
        let entry = Entry {
            since,
            since_estimated: seeded && (reported.is_none() || pane.state == "working"),
            turn_started: (pane.state == "working").then_some(since),
            last_turn: None,
            seen: !matches!(pane.state.as_str(), "done" | "blocked" | "error"),
            stale: false,
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
fn urgency(entry: &Entry) -> u8 {
    match entry.group() {
        Group::NeedsYou => pane::rank(&entry.pane.state),
        _ => 0,
    }
}

fn parse<T>(result: serde_json::Result<T>) -> Result<T> {
    result.map_err(|err| Error::Rex(format!("unexpected pane shape from Rex: {err}")))
}

fn time(at: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|err| Error::Rex(format!("updated_at {at:?} is not RFC 3339: {err}")))
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
            .rows()
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
        assert_eq!(row.group, Group::NoSignal);
        let gone = replay(21);
        assert!(gone.rows().iter().all(|r| r.pane.block_id != PANE));
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
            .rows()
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
    fn stale_host_rows_move_to_no_signal() {
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
        let rows = board.rows();
        let mini = rows.iter().find(|r| r.pane.host == "mini").unwrap();
        let studio = rows.iter().find(|r| r.pane.host == "studio").unwrap();
        assert!(mini.stale);
        assert_eq!(mini.group, Group::NoSignal);
        assert_eq!(studio.group, Group::NeedsYou);
    }

    #[test]
    fn updated_at_is_parsed_not_compared_as_text() {
        let mut board = Board::default();
        let blocks = json!([
            block("block:utc", "done", "2026-01-01T04:00:00Z"),
            block("block:ict", "done", "2026-01-01T10:00:00+07:00"),
        ]);
        board.apply("mini", &snapshot(blocks), at(0)).unwrap();
        let rows = board.rows();
        assert_eq!(rows[0].pane.block_id, "block:ict");
        assert_eq!(rows[0].since, "2026-01-01T03:00:00.000Z");
        let bad = json!([block("block:x", "done", "yesterday")]);
        let err = board.apply("mini", &snapshot(bad), at(0)).unwrap_err();
        assert_eq!(err.code(), "REX");
    }
}
