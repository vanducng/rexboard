//! What `pick` keeps in pick.json and the board in board.json: view and sort, and for the board
//! the status filter and `/` text. Both files are written whole through a temp file, with every
//! key they do not know kept.

use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use super::query::status_of;
use super::status::Status;

#[derive(Default, Clone, PartialEq, Debug)]
pub struct ViewSort {
    pub view: String,
    pub sort: Option<(String, bool)>,
}

/// One key to set, or to remove with None.
pub type Change = (&'static str, Option<Value>);

#[derive(Default, Clone, PartialEq, Debug)]
pub struct BoardPrefs {
    pub view_sort: ViewSort,
    pub status: Option<Status>,
    pub filter: String,
}

/// The file's keys, and why it was not used when it exists but holds no JSON object. A missing
/// file is the defaults and says nothing.
fn read(path: &Path) -> (Map<String, Value>, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Default::default(),
        Err(err) => return (Map::new(), Some(format!("unreadable: {err}"))),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(object)) => (object, None),
        Ok(_) => (Map::new(), Some("not a JSON object".to_owned())),
        Err(_) => (Map::new(), Some("not valid JSON".to_owned())),
    }
}

fn view_sort_of(object: &Map<String, Value>) -> ViewSort {
    let view = object
        .get("view")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let sort = object.get("sort").and_then(|value| {
        let sort = value.as_object()?;
        Some((
            sort.get("key")?.as_str()?.to_owned(),
            sort.get("descending")?.as_bool()?,
        ))
    });
    ViewSort { view, sort }
}

/// Read the picker settings. Missing, malformed, or incorrectly typed settings use defaults.
pub fn load(path: &Path) -> ViewSort {
    view_sort_of(&read(path).0)
}

/// Read the board's settings, and the one thing to tell the user when the file cannot be used.
pub fn load_board(path: &Path) -> (BoardPrefs, Option<String>) {
    let (object, note) = read(path);
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .and_then(|text| status_of(text).ok());
    let filter = object
        .get("filter")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let prefs = BoardPrefs {
        view_sort: view_sort_of(&object),
        status,
        filter,
    };
    (prefs, note)
}

fn sort_value(sort: &Option<(String, bool)>) -> Option<Value> {
    sort.as_ref()
        .map(|(key, descending)| serde_json::json!({"key": key, "descending": descending}))
}

impl ViewSort {
    fn changes(&self) -> Vec<Change> {
        vec![
            ("view", Some(Value::String(self.view.clone()))),
            ("sort", sort_value(&self.sort)),
        ]
    }
}

impl BoardPrefs {
    /// The keys that differ in `now`: a board writes only what its user changed, so two boards
    /// changing different settings keep both.
    pub fn changes(&self, now: &Self) -> Vec<Change> {
        let mut changes = Vec::new();
        if self.view_sort.view != now.view_sort.view {
            changes.push(("view", Some(Value::String(now.view_sort.view.clone()))));
        }
        if self.view_sort.sort != now.view_sort.sort {
            changes.push(("sort", sort_value(&now.view_sort.sort)));
        }
        if self.status != now.status {
            changes.push((
                "status",
                now.status.map(|s| Value::String(s.name().to_owned())),
            ));
        }
        if self.filter != now.filter {
            let filter = Some(&now.filter).filter(|f| !f.is_empty());
            changes.push(("filter", filter.map(|f| Value::String(f.clone()))));
        }
        changes
    }
}

/// Save picker settings while retaining keys owned by other versions or callers.
pub fn save(path: &Path, prefs: &ViewSort) -> io::Result<()> {
    save_changes(path, &prefs.changes())
}

/// Set or remove these keys in the file as it is now, keeping the rest. A file that holds no JSON
/// object is replaced.
pub fn save_changes(path: &Path, changes: &[Change]) -> io::Result<()> {
    let mut object = read(path).0;
    for (key, value) in changes {
        match value {
            Some(value) => {
                object.insert((*key).to_owned(), value.clone());
            }
            None => {
                object.remove(*key);
            }
        }
    }
    let text = serde_json::to_vec_pretty(&Value::Object(object))?;

    super::prefs::write_whole(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "rexboard-pick-prefs-{}-{nonce}.json",
                std::process::id()
            )))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_or_invalid_data_uses_defaults() {
        let file = TempFile::new();
        assert_eq!(load(&file.0), ViewSort::default());
        std::fs::write(&file.0, "not json").unwrap();
        assert_eq!(load(&file.0), ViewSort::default());
        std::fs::write(&file.0, "[]").unwrap();
        assert_eq!(load(&file.0), ViewSort::default());
    }

    #[test]
    fn malformed_fields_are_ignored_independently() {
        let file = TempFile::new();
        std::fs::write(
            &file.0,
            r#"{"view":4,"sort":{"key":"name","descending":"no"}}"#,
        )
        .unwrap();
        assert_eq!(load(&file.0), ViewSort::default());
        std::fs::write(
            &file.0,
            r#"{"view":"recent","sort":{"key":"name","descending":true}}"#,
        )
        .unwrap();
        assert_eq!(
            load(&file.0),
            ViewSort {
                view: "recent".into(),
                sort: Some(("name".into(), true))
            }
        );
    }

    #[test]
    fn save_preserves_unknown_object_keys() {
        let file = TempFile::new();
        std::fs::write(
            &file.0,
            r#"{"future":{"enabled":true},"view":"old","other":9}"#,
        )
        .unwrap();
        let prefs = ViewSort {
            view: "wide".into(),
            sort: Some(("field-id".into(), false)),
        };
        save(&file.0, &prefs).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file.0).unwrap()).unwrap();
        assert_eq!(saved["future"], serde_json::json!({"enabled":true}));
        assert_eq!(saved["other"], 9);
        assert_eq!(load(&file.0), prefs);
    }

    #[test]
    fn save_creates_the_directory() {
        let file = TempFile::new();
        let path = file.0.with_extension("d").join("pick.json");
        save(&path, &ViewSort::default()).unwrap();
        assert!(path.exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_board_reads_all_four_and_says_why_a_file_is_not_used() {
        let file = TempFile::new();
        assert_eq!(load_board(&file.0), (BoardPrefs::default(), None));
        std::fs::write(
            &file.0,
            r#"{"view":"host","sort":{"key":"builtin:a","descending":false},"status":"marked","filter":"web"}"#,
        )
        .unwrap();
        let (prefs, note) = load_board(&file.0);
        assert_eq!(note, None);
        assert_eq!(prefs.status, Some(Status::Marked));
        assert_eq!(prefs.filter, "web");
        assert_eq!(prefs.view_sort.view, "host");
        for (text, why) in [("{nope", "not valid JSON"), ("[]", "not a JSON object")] {
            std::fs::write(&file.0, text).unwrap();
            assert_eq!(
                load_board(&file.0),
                (BoardPrefs::default(), Some(why.into()))
            );
        }
        std::fs::write(&file.0, r#"{"status":"nope","filter":3}"#).unwrap();
        assert_eq!(load_board(&file.0), (BoardPrefs::default(), None));
    }

    #[test]
    fn changes_name_only_the_keys_that_differ_and_a_clear_removes_its_key() {
        let before = BoardPrefs {
            status: Some(Status::Done),
            filter: "web".into(),
            ..Default::default()
        };
        assert!(before.changes(&before.clone()).is_empty());
        let now = BoardPrefs {
            status: None,
            filter: String::new(),
            ..before.clone()
        };
        assert_eq!(
            before.changes(&now),
            vec![("status", None), ("filter", None)]
        );
    }

    #[test]
    fn two_boards_changing_different_settings_keep_both_and_unknown_keys() {
        let file = TempFile::new();
        std::fs::write(&file.0, r#"{"future":1}"#).unwrap();
        let none = BoardPrefs::default();
        let status = BoardPrefs {
            status: Some(Status::Done),
            ..Default::default()
        };
        let filter = BoardPrefs {
            filter: "web".into(),
            ..Default::default()
        };
        save_changes(&file.0, &none.changes(&status)).unwrap();
        save_changes(&file.0, &none.changes(&filter)).unwrap();
        let (prefs, _) = load_board(&file.0);
        assert_eq!(
            (prefs.status, prefs.filter.as_str()),
            (Some(Status::Done), "web")
        );
        let saved: Value = serde_json::from_slice(&std::fs::read(&file.0).unwrap()).unwrap();
        assert_eq!(saved["future"], 1);
    }

    #[test]
    fn a_save_replaces_a_file_that_holds_no_object() {
        let file = TempFile::new();
        std::fs::write(&file.0, "{nope").unwrap();
        let status = BoardPrefs {
            status: Some(Status::Idle),
            ..Default::default()
        };
        save_changes(&file.0, &BoardPrefs::default().changes(&status)).unwrap();
        assert_eq!(load_board(&file.0), (status, None));
    }
}
