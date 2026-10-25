use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Major version of the observer.lua line contract.
pub const VERSION: u64 = 1;

/// One line from observer.lua. `record` and `sessions` keep Rex's own shapes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Snapshot {
        sessions: Value,
        rex_version: Option<String>,
    },
    Status {
        session_id: String,
        block_id: String,
        record_id: Option<String>,
        record: Value,
    },
    StatusRemoved {
        session_id: String,
        block_id: String,
        #[serde(default)]
        ids: Vec<String>,
        reason: Option<String>,
    },
    Title {
        session_id: String,
        block_id: String,
        title: Option<String>,
    },
    Process {
        session_id: String,
        block_id: String,
        name: Option<String>,
        cwd: Option<String>,
    },
    ChildExited {
        session_id: String,
        block_id: String,
        exit_code: Option<i64>,
    },
    BlockCreated {
        session_id: String,
        window_id: String,
        block: Value,
    },
    BlockClosed {
        session_id: String,
        block_id: String,
    },
    WindowCreated {
        session_id: String,
        window_id: String,
        label: Option<String>,
    },
    WindowClosed {
        session_id: String,
        window_id: String,
    },
    Label {
        kind: LabelKind,
        session_id: String,
        id: String,
        label: Option<String>,
    },
    ActiveWindow {
        session_id: String,
        window_id: Option<String>,
    },
    SessionCreated {
        session_id: String,
        label: Option<String>,
    },
    SessionDestroyed {
        session_id: String,
    },
    Tick,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    Session,
    Window,
    Block,
}

/// Parses one observer line. `rex_version` names the Rex build in a contract error.
pub fn parse(line: &str, rex_version: &str) -> Result<Event> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Rex(format!("observer printed a line that is not JSON: {err}")))?;
    match value.get("v").and_then(Value::as_u64) {
        Some(VERSION) => {}
        v => {
            return Err(Error::ContractMismatch(format!(
                "observer line version {} is not {VERSION} (Rex {rex_version})",
                v.map_or("missing".into(), |v| v.to_string())
            )));
        }
    }
    serde_json::from_value(value).map_err(|err| {
        Error::Rex(format!(
            "unexpected observer line from Rex {rex_version}: {err}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_unknown_type_is_ignored() {
        let event = parse(
            r#"{"v":1,"type":"bell","session_id":"s","extra":1}"#,
            "test",
        )
        .unwrap();
        assert_eq!(event, Event::Unknown);
    }

    #[test]
    fn event_rejects_unknown_major_version() {
        let err = parse(r#"{"v":2,"type":"tick"}"#, "0.0.1-gabc").unwrap_err();
        assert_eq!(err.code(), "CONTRACT_MISMATCH");
        assert!(err.to_string().contains("Rex 0.0.1-gabc"), "{err}");
        assert!(parse(r#"{"type":"tick"}"#, "x").is_err());
    }

    #[test]
    fn event_known_type_with_a_bad_field_is_an_error() {
        let err = parse(r#"{"v":1,"type":"title","session_id":"s"}"#, "x").unwrap_err();
        assert_eq!(err.code(), "REX");
    }

    // Field names recorded live from Rex 0.0.1792787366-g623b2d session events.
    #[test]
    fn event_session_shapes_match_rex() {
        let label = parse(
            r#"{"v":1,"type":"label","kind":"window","session_id":"session:a","id":"window:b","label":"two"}"#,
            "x",
        )
        .unwrap();
        assert_eq!(
            label,
            Event::Label {
                kind: LabelKind::Window,
                session_id: "session:a".into(),
                id: "window:b".into(),
                label: Some("two".into()),
            }
        );
        let active = parse(
            r#"{"v":1,"type":"active_window","session_id":"session:a","window_id":"window:b"}"#,
            "x",
        )
        .unwrap();
        assert_eq!(
            active,
            Event::ActiveWindow {
                session_id: "session:a".into(),
                window_id: Some("window:b".into()),
            }
        );
    }
}
