use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{Error as _, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    #[serde(default, deserialize_with = "seq")]
    pub hosts: Vec<HostSnap>,
}

#[derive(Debug, Deserialize)]
pub struct HostSnap {
    pub host: String,
    pub error: Option<String>,
    #[serde(default)]
    pub sessions: Value,
}

#[derive(Debug, Deserialize)]
pub struct SessionSnap {
    pub session_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, deserialize_with = "seq")]
    pub windows: Vec<WindowSnap>,
}

#[derive(Debug, Deserialize)]
pub struct WindowSnap {
    pub window_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, deserialize_with = "seq")]
    pub blocks: Vec<BlockSnap>,
}

#[derive(Debug, Deserialize)]
pub struct BlockSnap {
    block_id: String,
    #[serde(default)]
    label: String,
    #[serde(default, deserialize_with = "seq")]
    records: Vec<Record>,
    title: Option<String>,
    #[serde(default)]
    process: Process,
}

#[derive(Debug, Default, Deserialize)]
struct Process {
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Record {
    id: Option<String>,
    pub state: String,
    kind: Option<String>,
    msg: Option<String>,
    app: Option<String>,
    effective_app: Option<String>,
    owner: Option<Owner>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Owner {
    name: Option<String>,
}

/// What Rex reports for one pane; a `Pane` is derived from it.
#[derive(Debug, Clone)]
pub struct Source {
    pub session_id: String,
    pub window_id: String,
    pub block_id: String,
    pub label: String,
    pub title: String,
    pub process: Option<String>,
    /// Keyed by record id; `""` is the pane's main record.
    pub records: BTreeMap<String, Record>,
    /// An extension's status and its name, used only while the pane has no record of its own.
    pub fallback: Option<(String, crate::extension::contract::Status)>,
}

impl Source {
    pub fn new(session_id: &str, window_id: &str, block: BlockSnap) -> Self {
        Self {
            session_id: session_id.to_owned(),
            window_id: window_id.to_owned(),
            block_id: block.block_id,
            label: block.label,
            title: block.title.unwrap_or_default(),
            process: block.process.name,
            records: block
                .records
                .into_iter()
                .map(|r| (r.id.clone().unwrap_or_default(), r))
                .collect(),
            fallback: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub host: String,
    pub session_id: String,
    pub session: String,
    pub window_id: String,
    pub tab: String,
    pub block_id: String,
    pub pane: String,
    pub title: String,
    /// The foreground process, as in `zsh` or `tmux`.
    pub process: Option<String>,
    pub harness: Option<String>,
    pub state: String,
    pub kind: Option<String>,
    pub msg: Option<String>,
    pub updated_at: Option<String>,
    pub source: String,
    pub children: BTreeMap<String, usize>,
}

impl Pane {
    /// A pane with a status record or a known harness; the rest are plain shells.
    pub fn is_agent(&self) -> bool {
        self.source != "inferred" || self.harness.is_some()
    }
}

pub fn parse_sessions(sessions: &Value) -> serde_json::Result<Vec<SessionSnap>> {
    #[derive(Deserialize)]
    struct Sessions(#[serde(deserialize_with = "seq")] Vec<SessionSnap>);
    Sessions::deserialize(sessions).map(|s| s.0)
}

pub fn parse_block(block: &Value) -> serde_json::Result<BlockSnap> {
    BlockSnap::deserialize(block)
}

pub fn parse_record(record: &Value) -> serde_json::Result<Record> {
    Record::deserialize(record)
}

/// Lua encodes an empty table as `{}`, so an empty list can arrive as an object. Anything else is drift.
fn seq<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    struct Seq<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Seq<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a list, or {} for an empty one")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(items)
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<T>, A::Error> {
            match map.next_key::<IgnoredAny>()? {
                None => Ok(Vec::new()),
                Some(_) => Err(A::Error::invalid_type(Unexpected::Map, &self)),
            }
        }
    }
    d.deserialize_any(Seq(PhantomData))
}

pub fn rank(state: &str) -> u8 {
    match state {
        "blocked" => 0,
        "error" => 1,
        "done" => 2,
        "working" => 3,
        "idle" => 4,
        _ => 5,
    }
}

pub fn derive(host: &str, session: &str, tab: &str, src: &Source) -> Pane {
    let turn = src.records.get(TURN);
    let others = || {
        src.records
            .iter()
            .filter(|(id, _)| *id != TURN)
            .map(|(_, r)| r)
    };
    let main = src
        .records
        .get("")
        .or_else(|| others().min_by_key(|r| rank(&r.state)));
    let mut children = BTreeMap::new();
    for record in others() {
        if !main.is_some_and(|m| std::ptr::eq(m, record)) {
            *children.entry(record.state.clone()).or_insert(0) += 1;
        }
    }
    let process = src.process.as_deref();
    let harness = crate::harness::name_of(
        main.and_then(|r| r.effective_app.clone().or_else(|| r.app.clone())),
        process,
        &src.title,
    );
    let owner = main
        .and_then(|r| r.owner.as_ref())
        .and_then(|o| o.name.as_deref());
    // Native records always win over an extension's status.
    let fallback = src.fallback.as_ref().filter(|_| src.records.is_empty());
    let source = match main {
        None => fallback.map_or("inferred", |(name, _)| name.as_str()),
        Some(_) if owner == Some("tmux") || process == Some("tmux") => "bridge",
        Some(_) => "native",
    }
    .to_owned();
    let (state, kind, msg, updated_at) = match main {
        Some(r) => (
            r.state.clone(),
            r.kind.clone(),
            r.msg
                .clone()
                .filter(|msg| !msg.is_empty())
                .or_else(|| turn_msg(r, turn)),
            r.updated_at.clone(),
        ),
        None => match fallback {
            Some((_, status)) => (
                status.state.clone(),
                status.kind.clone(),
                status.msg.clone().filter(|msg| !msg.is_empty()),
                None,
            ),
            None => {
                let state = title_state(&src.title).unwrap_or("unknown");
                (state.to_owned(), None, None, None)
            }
        },
    };
    Pane {
        host: host.to_owned(),
        session_id: src.session_id.clone(),
        session: session.to_owned(),
        window_id: src.window_id.clone(),
        tab: tab.to_owned(),
        block_id: src.block_id.clone(),
        pane: src.label.clone(),
        title: src.title.clone(),
        process: src.process.clone(),
        harness,
        state,
        kind,
        msg,
        updated_at,
        source,
        children,
    }
}

/// The sub-record a hook writes beside an agent's own status in a plain Rex pane, to carry the
/// turn's final message that the agent's own `done` leaves out.
const TURN: &str = "turn";

/// The `turn` record's message. A `done` root always takes it: the agent's own `done` lands just
/// after the hook's `turn`, and the hook clears `turn` when the next prompt starts. Any other root
/// takes it only if it is not older, so a stale turn never speaks for a newer state.
fn turn_msg(root: &Record, turn: Option<&Record>) -> Option<String> {
    let turn = turn?;
    let at = |r: &Record| {
        r.updated_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
    };
    match (at(root), at(turn)) {
        (Some(root_at), Some(turn_at)) if turn_at < root_at && root.state != "done" => None,
        _ => turn.msg.clone().filter(|msg| !msg.is_empty()),
    }
}

/// Claude Code and the seat status line put a state glyph at the start of the title.
fn glyph(title: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = title.strip_prefix("✳ ") {
        return Some(("idle", rest));
    }
    let first = title.chars().next()?;
    match first {
        '\u{2800}'..='\u{28FF}' | '◐' | '◑' => {
            let rest = &title[first.len_utf8()..];
            Some(("working", rest.strip_prefix(' ').unwrap_or(rest)))
        }
        _ => None,
    }
}

pub(crate) fn title_state(title: &str) -> Option<&'static str> {
    glyph(title).map(|(state, _)| state)
}

/// The title without its leading state glyph, for display. The seat status line's other
/// glyphs are dropped too, though they do not set a state.
pub fn bare_title(title: &str) -> &str {
    if let Some((_, rest)) = glyph(title) {
        return rest;
    }
    ["◉ ", "● ", "○ ", "✖ "]
        .iter()
        .find_map(|g| title.strip_prefix(g))
        .unwrap_or(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_records(records: serde_json::Value) -> Pane {
        let block = parse_block(&serde_json::json!({
            "block_id": "b", "label": "seat", "title": "t", "records": records,
        }))
        .unwrap();
        derive("local", "api", "main", &Source::new("s", "w", block))
    }

    #[test]
    fn a_turn_record_fills_an_empty_msg_and_is_no_child() {
        // Claude's own `done` lands about 40 ms after the hook's `turn`.
        let pane = with_records(serde_json::json!([
            {"state": "done", "app": "claude-code", "updated_at": "2026-01-01T00:00:00.040Z"},
            {"id": "turn", "state": "done", "app": "claude-code", "msg": "Opened PR #31",
             "updated_at": "2026-01-01T00:00:00Z"},
        ]));
        assert_eq!(
            (pane.state.as_str(), pane.msg.as_deref()),
            ("done", Some("Opened PR #31"))
        );
        assert!(pane.children.is_empty(), "{:?}", pane.children);
        let older = with_records(serde_json::json!([
            {"state": "working", "app": "claude-code", "updated_at": "2026-01-01T00:05:00Z"},
            {"id": "turn", "state": "done", "msg": "the last turn", "updated_at": "2026-01-01T00:00:01Z"},
        ]));
        assert_eq!(older.msg, None, "a turn older than the root says nothing");
        let own = with_records(serde_json::json!([
            {"state": "blocked", "msg": "Allow Bash?", "updated_at": "2026-01-01T00:00:00Z"},
            {"id": "turn", "state": "done", "msg": "the last turn", "updated_at": "2026-01-01T00:00:01Z"},
        ]));
        assert_eq!(
            own.msg.as_deref(),
            Some("Allow Bash?"),
            "the root's own msg wins"
        );
        let alone = with_records(serde_json::json!([
            {"id": "turn", "state": "done", "msg": "the last turn"},
        ]));
        assert_eq!(
            alone.source, "inferred",
            "a turn record alone is not the pane's status"
        );
    }

    #[test]
    fn title_glyphs_map_to_states() {
        assert_eq!(title_state("✳ seat-a@demo"), Some("idle"));
        assert_eq!(title_state("⠋ seat-a@demo"), Some("working"));
        assert_eq!(title_state("◑ seat-a@demo"), Some("working"));
        assert_eq!(title_state("✳seat"), None);
        assert_eq!(title_state(""), None);
    }

    #[test]
    fn bare_title_drops_the_glyph_and_its_space() {
        assert_eq!(bare_title("✳ seat-a@demo"), "seat-a@demo");
        assert_eq!(bare_title("⠋ seat-a@demo"), "seat-a@demo");
        assert_eq!(bare_title("◐ seat-a@demo"), "seat-a@demo");
        for g in ["◉", "●", "○", "✖"] {
            assert_eq!(bare_title(&format!("{g} seat-a@demo")), "seat-a@demo");
            assert_eq!(
                title_state(&format!("{g} seat-a@demo")),
                None,
                "no state from {g}"
            );
        }
        assert_eq!(bare_title("✳seat-a"), "✳seat-a");
        assert_eq!(bare_title("Fix the parser"), "Fix the parser");
    }

    #[test]
    fn seq_rejects_anything_but_a_list_or_empty_object() {
        let parse = |json: &str| serde_json::from_str::<Snapshot>(json).map(|s| s.hosts.len());
        assert_eq!(parse(r#"{"hosts": {}}"#).unwrap(), 0);
        assert_eq!(parse(r#"{"hosts": [{"host": "local"}]}"#).unwrap(), 1);
        assert!(parse(r#"{"hosts": {"a": 1}}"#).is_err());
        assert!(parse(r#"{"hosts": "nope"}"#).is_err());
        assert!(parse(r#"{"hosts": [{"host": 1}]}"#).is_err());
    }
}

/// Today's detection, pinned row by row, so moving it changes no harness, source or state.
#[cfg(test)]
mod corpus {
    use super::*;

    #[test]
    fn derive_matches_the_recorded_corpus() {
        let apps: [Option<(&str, &str)>; 8] = [
            None,
            Some(("app", "claude-code")),
            Some(("app", "codex")),
            Some(("app", "pi")),
            Some(("app", "grok-build")),
            Some(("app", "opencode")),
            Some(("effective_app", "codex")),
            Some(("app", "")),
        ];
        let processes = [
            None,
            Some("claude"),
            Some("codex"),
            Some("pi"),
            Some("2.1.310"),
            Some("2.1"),
            Some("10.20.30"),
            Some("tmux"),
            Some("zsh"),
            Some("node"),
            Some("grok"),
            Some("grok-1.0.70-darwin-arm64"),
            Some("rexboard"),
        ];
        let titles = [
            "",
            "plain",
            "✳ idle seat",
            "⠋ working seat",
            "⣿ busy",
            "◐ half",
            "◑ half",
            "◉ seat",
            "api | gpt-5.1",
            "api | gpt5",
            "env | sort",
            "a b | gpt-5",
            "api | model",
            "api | gpt-5 | x",
        ];
        let owners = [None, Some("tmux")];
        let mut out = String::new();
        for app in apps {
            for process in processes {
                for title in titles {
                    for owner in owners {
                        if owner.is_some() && app.is_none() {
                            continue;
                        }
                        let mut record = serde_json::json!({ "state": "working" });
                        if let Some((key, value)) = app {
                            record[key] = serde_json::json!(value);
                        }
                        if let Some(owner) = owner {
                            record["owner"] = serde_json::json!({ "name": owner });
                        }
                        let records = if app.is_some() || owner.is_some() {
                            serde_json::json!([record])
                        } else {
                            serde_json::json!([])
                        };
                        let mut block = serde_json::json!({
                            "block_id": "b", "label": "seat", "title": title, "records": records,
                        });
                        if let Some(process) = process {
                            block["process"] = serde_json::json!({ "name": process });
                        }
                        let pane = derive(
                            "local",
                            "api",
                            "main",
                            &Source::new("s", "w", parse_block(&block).unwrap()),
                        );
                        out.push_str(&format!(
                            "{app:?} {process:?} {title:?} {owner:?} => {:?} {} {}\n",
                            pane.harness, pane.source, pane.state
                        ));
                    }
                }
            }
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/harness/derive.txt"
        );
        if std::env::var_os("UPDATE_HARNESS_CORPUS").is_some() {
            std::fs::write(path, &out).unwrap();
        }
        assert_eq!(out, std::fs::read_to_string(path).expect("the corpus"));
    }
}
