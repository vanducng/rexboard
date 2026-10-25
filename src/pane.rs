use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    #[serde(default, deserialize_with = "seq")]
    pub hosts: Vec<HostSnap>,
}

#[derive(Debug, Deserialize)]
pub struct HostSnap {
    pub host: String,
    pub error: Option<String>,
    #[serde(default, deserialize_with = "seq")]
    sessions: Vec<SessionSnap>,
}

#[derive(Debug, Deserialize)]
struct SessionSnap {
    session_id: String,
    #[serde(default)]
    label: String,
    #[serde(default, deserialize_with = "seq")]
    windows: Vec<WindowSnap>,
}

#[derive(Debug, Deserialize)]
struct WindowSnap {
    window_id: String,
    #[serde(default)]
    label: String,
    #[serde(default, deserialize_with = "seq")]
    blocks: Vec<BlockSnap>,
}

#[derive(Debug, Deserialize)]
struct BlockSnap {
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

#[derive(Debug, Deserialize)]
struct Record {
    id: Option<String>,
    state: String,
    kind: Option<String>,
    msg: Option<String>,
    app: Option<String>,
    effective_app: Option<String>,
    owner: Option<Owner>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Pane {
    pub host: String,
    pub session_id: String,
    pub session: String,
    pub window_id: String,
    pub tab: String,
    pub block_id: String,
    pub pane: String,
    pub title: String,
    pub harness: Option<String>,
    pub state: String,
    pub kind: Option<String>,
    pub msg: Option<String>,
    pub updated_at: Option<String>,
    pub source: &'static str,
    pub children: BTreeMap<String, usize>,
}

/// Lua encodes an empty table as `{}`, so an empty list can arrive as an object.
fn seq<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seq<T> {
        List(Vec<T>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Seq::deserialize(d)? {
        Seq::List(items) => items,
        Seq::Other(_) => Vec::new(),
    })
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

pub fn flatten(hosts: Vec<HostSnap>, all: bool) -> Vec<Pane> {
    let mut panes = Vec::new();
    for host in hosts {
        for mut session in host.sessions {
            for mut window in std::mem::take(&mut session.windows) {
                for block in std::mem::take(&mut window.blocks) {
                    let pane = pane(&host.host, &session, &window, block);
                    if all || pane.source != "inferred" || pane.harness.is_some() {
                        panes.push(pane);
                    }
                }
            }
        }
    }
    panes.sort_by_key(|pane| rank(&pane.state));
    panes
}

fn pane(host: &str, session: &SessionSnap, window: &WindowSnap, block: BlockSnap) -> Pane {
    let title = block.title.unwrap_or_default();
    let mut records = block.records;
    let main = records
        .iter()
        .position(|r| r.id.as_deref().unwrap_or("").is_empty())
        .or_else(|| (0..records.len()).min_by_key(|&i| rank(&records[i].state)))
        .map(|i| records.swap_remove(i));
    let mut children = BTreeMap::new();
    for record in &records {
        *children.entry(record.state.clone()).or_insert(0) += 1;
    }
    let process = block.process.name.as_deref();
    let harness = main
        .as_ref()
        .and_then(|r| r.effective_app.clone().or_else(|| r.app.clone()))
        .or_else(|| process.and_then(process_harness))
        .or_else(|| title_state(&title).map(|_| "claude-code".to_owned()));
    let owner = main
        .as_ref()
        .and_then(|r| r.owner.as_ref())
        .and_then(|o| o.name.as_deref());
    let source = match &main {
        None => "inferred",
        Some(_) if owner == Some("tmux") || process == Some("tmux") => "bridge",
        Some(_) => "native",
    };
    let (state, kind, msg, updated_at) = match main {
        Some(r) => (r.state, r.kind, r.msg, r.updated_at),
        None => {
            let state = title_state(&title).unwrap_or("unknown");
            (state.to_owned(), None, None, None)
        }
    };
    Pane {
        host: host.to_owned(),
        session_id: session.session_id.clone(),
        session: session.label.clone(),
        window_id: window.window_id.clone(),
        tab: window.label.clone(),
        block_id: block.block_id,
        pane: block.label,
        title,
        harness,
        state,
        kind,
        msg,
        updated_at,
        source,
        children,
    }
}

fn process_harness(name: &str) -> Option<String> {
    let is_version = name.split('.').count() == 3
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    match name {
        "claude" => Some("claude-code".to_owned()),
        "codex" | "pi" => Some(name.to_owned()),
        _ if is_version => Some("claude-code".to_owned()),
        _ => None,
    }
}

/// Claude Code and the seat status line put a state glyph at the start of the title.
fn title_state(title: &str) -> Option<&'static str> {
    if title.starts_with("✳ ") {
        return Some("idle");
    }
    match title.chars().next()? {
        '\u{2800}'..='\u{28FF}' | '◐' | '◑' => Some("working"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_process_is_claude() {
        assert_eq!(process_harness("2.1.310").as_deref(), Some("claude-code"));
        assert_eq!(process_harness("2.1"), None);
        assert_eq!(process_harness("zsh"), None);
    }

    #[test]
    fn title_glyphs_map_to_states() {
        assert_eq!(title_state("✳ seat-a@demo"), Some("idle"));
        assert_eq!(title_state("⠋ seat-a@demo"), Some("working"));
        assert_eq!(title_state("◑ seat-a@demo"), Some("working"));
        assert_eq!(title_state("✳seat"), None);
        assert_eq!(title_state(""), None);
    }
}
