//! Contract 1: the request goes to the adapter's stdin, and fields per row, plus an optional
//! fallback status, come back on its stdout.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{CONTRACT, Extension};
use crate::board::Row;

/// The most an adapter may print.
pub const MAX_OUTPUT: usize = 1 << 20;
/// The longest field value, in characters.
pub const MAX_VALUE: usize = 128;
/// The longest status message, as Rex caps its own.
pub const MAX_MSG: usize = 2048;
pub const STATES: [&str; 5] = ["idle", "working", "blocked", "done", "error"];

#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub contract: u32,
    pub extension: &'a str,
    pub settings: &'a Value,
    pub rows: &'a [RowIn],
}

/// What an adapter learns about a row; the same names as `ls --json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowIn {
    pub key: String,
    pub host: String,
    pub session: String,
    pub tab: String,
    pub pane: String,
    pub title: String,
    pub process: Option<String>,
    pub harness: Option<String>,
    pub state: String,
    pub source: String,
}

impl From<&Row> for RowIn {
    fn from(row: &Row) -> Self {
        let p = &row.pane;
        Self {
            key: row.key.clone(),
            host: p.host.clone(),
            session: p.session.clone(),
            tab: p.tab.clone(),
            pane: p.pane.clone(),
            title: p.title.clone(),
            process: p.process.clone(),
            harness: p.harness.clone(),
            state: p.state.clone(),
            source: p.source.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    contract: u32,
    #[serde(default)]
    rows: BTreeMap<String, RowOut>,
}

#[derive(Debug, Deserialize)]
struct RowOut {
    #[serde(default)]
    fields: BTreeMap<String, Value>,
    #[serde(default)]
    status: Option<Status>,
}

/// A fallback status, for a row with no native record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
}

/// One row's answer: field values by the field's short name, and its status.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub fields: BTreeMap<String, String>,
    pub status: Option<Status>,
}

/// A valid response, and each part of it that was dropped and why.
#[derive(Debug, Default)]
pub struct Parsed {
    pub rows: BTreeMap<String, Item>,
    pub problems: Vec<String>,
}

/// Reads an adapter's stdout against the rows it was sent. A response over the size limit, not
/// JSON or for another contract fails whole; anything else wrong is dropped and named.
pub fn parse(out: &[u8], ext: &Extension, keys: &BTreeSet<String>) -> Result<Parsed, String> {
    if out.len() > MAX_OUTPUT {
        return Err(format!("printed more than {MAX_OUTPUT} bytes"));
    }
    let response: Response =
        serde_json::from_slice(out).map_err(|err| format!("printed bad JSON: {err}"))?;
    if response.contract != CONTRACT {
        return Err(format!(
            "answered for contract {}; this rexboard speaks contract {CONTRACT}",
            response.contract
        ));
    }
    let mut parsed = Parsed::default();
    for (key, row) in response.rows {
        if !keys.contains(&key) {
            parsed.problems.push(format!("unknown row {key}"));
            continue;
        }
        let mut item = Item::default();
        for (name, value) in row.fields {
            match value {
                _ if !ext.declares(&name) => {
                    parsed.problems.push(format!("undeclared field {name}"));
                }
                Value::String(text) if text.chars().count() <= MAX_VALUE => {
                    item.fields.insert(name, text);
                }
                Value::String(_) => parsed
                    .problems
                    .push(format!("field {name} is over {MAX_VALUE} characters")),
                Value::Null => {}
                _ => parsed
                    .problems
                    .push(format!("field {name} is not a string")),
            }
        }
        item.status = match row
            .status
            .map(|status| (status_problem(&status, ext), status))
        {
            Some((None, status)) => Some(status),
            Some((Some(why), _)) => {
                parsed.problems.push(why);
                None
            }
            None => None,
        };
        parsed.rows.insert(key, item);
    }
    Ok(parsed)
}

/// Why a row's status cannot be used, if it cannot.
fn status_problem(status: &Status, ext: &Extension) -> Option<String> {
    if !ext.manifest.provides.status {
        return Some("a status, but the manifest does not set provides.status".to_owned());
    }
    if !STATES.contains(&status.state.as_str()) {
        return Some(format!(
            "status state {:?}, not one of {}",
            status.state,
            STATES.join(" ")
        ));
    }
    let long_kind = status
        .kind
        .as_ref()
        .is_some_and(|k| k.chars().count() > MAX_VALUE);
    let long_msg = status.msg.as_ref().is_some_and(|m| m.len() > MAX_MSG);
    (long_kind || long_msg).then(|| "a status kind or msg over its limit".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::tests::{manifest, write};

    fn ext(provides_status: bool) -> Extension {
        let root = tempfile::tempdir().unwrap();
        let mut text = manifest("fake", &["rig"]);
        if provides_status {
            text.push_str("[provides]\nstatus = true\n");
        }
        let dir = write(root.path(), "fake", &text);
        crate::extension::load(&dir).unwrap()
    }

    fn keys() -> BTreeSet<String> {
        BTreeSet::from(["local/b1".to_owned()])
    }

    #[test]
    fn a_response_keeps_declared_fields_and_names_what_it_drops() {
        let out = br#"{"contract": 1, "rows": {
            "local/b1": {"fields": {"rig": "api", "other": "x"},
                         "status": {"state": "idle", "kind": null, "msg": null}},
            "local/nope": {"fields": {"rig": "web"}}}}"#;
        let parsed = parse(out, &ext(true), &keys()).unwrap();
        let item = &parsed.rows["local/b1"];
        assert_eq!(item.fields, BTreeMap::from([("rig".into(), "api".into())]));
        assert_eq!(item.status.as_ref().unwrap().state, "idle");
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(
            parsed.problems,
            ["undeclared field other", "unknown row local/nope"]
        );
    }

    #[test]
    fn a_status_needs_provides_status_and_a_core_state() {
        let out = br#"{"contract": 1, "rows": {"local/b1": {"status": {"state": "idle"}}}}"#;
        let parsed = parse(out, &ext(false), &keys()).unwrap();
        assert_eq!(parsed.rows["local/b1"].status, None);
        assert!(parsed.problems[0].contains("provides.status"));
        let out = br#"{"contract": 1, "rows": {"local/b1": {"status": {"state": "asleep"}}}}"#;
        let parsed = parse(out, &ext(true), &keys()).unwrap();
        assert_eq!(parsed.rows["local/b1"].status, None);
        assert!(parsed.problems[0].contains("\"asleep\""));
    }

    #[test]
    fn a_long_value_is_dropped_and_a_bad_response_fails_whole() {
        let long = "x".repeat(MAX_VALUE + 1);
        let out = format!(
            r#"{{"contract": 1, "rows": {{"local/b1": {{"fields": {{"rig": "{long}"}}}}}}}}"#
        );
        let parsed = parse(out.as_bytes(), &ext(false), &keys()).unwrap();
        assert!(parsed.rows["local/b1"].fields.is_empty());
        assert_eq!(parsed.problems, ["field rig is over 128 characters"]);
        let e = ext(false);
        assert!(
            parse(b"not json", &e, &keys())
                .unwrap_err()
                .starts_with("printed bad JSON")
        );
        assert_eq!(
            parse(br#"{"contract": 2}"#, &e, &keys()).unwrap_err(),
            "answered for contract 2; this rexboard speaks contract 1"
        );
        let big = vec![b' '; MAX_OUTPUT + 1];
        assert!(parse(&big, &e, &keys()).unwrap_err().contains("more than"));
    }
}
