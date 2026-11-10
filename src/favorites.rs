//! Favorites: the panes the user marked with `*`, kept in favorites.json (decision 18).
//!
//! A pane is known by its labels, never by a block id, which Rex hands out again after a restart:
//! host, session, tab and pane label.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::board::Row;

/// What a row's `marked_by` says when the user marked it.
pub const FAVORITE: &str = "favorite";

/// An entry whose pane has not been on the board for this long goes at the next save.
const KEEP_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Id {
    pub host: String,
    pub session: String,
    pub tab: String,
    pub pane: String,
}

impl Id {
    /// None for a pane with no label of its own: nothing names it but its place.
    pub fn of(row: &Row) -> Option<Id> {
        let pane = &row.pane;
        (!pane.pane.is_empty()).then(|| Id {
            host: pane.host.clone(),
            session: pane.session.clone(),
            tab: pane.tab.clone(),
            pane: pane.pane.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    id: Id,
    /// Unix seconds when a save last found its pane on the board, or when it was added.
    seen: i64,
}

/// The file as it was read: the entries, and any key a later version added.
#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    favorites: Vec<Entry>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// The file, or why it cannot be used. A missing file is no favorites and no problem.
fn read(path: &Path) -> Result<File, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };
    serde_json::from_str(&text)
        .map_err(|err| format!("{}: not a favorites file: {err}", path.display()))
}

/// The favorites in force.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Favorites(Vec<Id>);

impl Favorites {
    /// What the file holds; a file that cannot be read gives none and says why.
    pub fn load(path: &Path) -> (Favorites, Option<String>) {
        match read(path) {
            Ok(file) => (
                Favorites(file.favorites.into_iter().map(|e| e.id).collect()),
                None,
            ),
            Err(why) => (Favorites::default(), Some(why)),
        }
    }

    /// Adds `favorite` last among the sources of the rows these name, and marks them. Every pane
    /// with the same labels in a tab matches. Entries no row matches stay dormant.
    pub fn apply(&self, rows: &mut [Row]) {
        for row in rows {
            row.marked_by.retain(|source| source != FAVORITE);
            if Id::of(row).is_some_and(|id| self.0.contains(&id)) {
                row.marked_by.push(FAVORITE.into());
            }
            row.marked = !row.marked_by.is_empty();
        }
    }
}

/// Marks `id` a favorite, or not, in the file at `path`. The file is read again first and only
/// this change is made, so a toggle never undoes another; `live` are the panes on the board now,
/// whose entries are seen, and the entries not seen for 30 days are dropped. A file that cannot
/// be read is left alone: it is the user's list.
pub fn set(path: &Path, id: &Id, on: bool, live: &[Id], now: i64) -> Result<(), String> {
    let mut file = read(path).map_err(|why| format!("{why}; move it aside to start a new list"))?;
    for entry in &mut file.favorites {
        if live.contains(&entry.id) {
            entry.seen = now;
        }
    }
    let at = file.favorites.iter().position(|entry| entry.id == *id);
    match (on, at) {
        (true, None) => file.favorites.push(Entry {
            id: id.clone(),
            seen: now,
        }),
        (true, Some(at)) => file.favorites[at].seen = now,
        (false, Some(at)) => {
            file.favorites.remove(at);
        }
        (false, None) => {}
    }
    file.favorites.retain(|entry| now - entry.seen <= KEEP_SECS);
    write(path, &file).map_err(|err| format!("{}: {err}", path.display()))
}

fn write(path: &Path, file: &File) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
    text.push('\n');
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    let written = out.write_all(text.as_bytes());
    drop(out);
    let done = written
        .and_then(|()| fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)))
        .and_then(|()| fs::rename(&temp, path));
    if done.is_err() {
        let _ = fs::remove_file(&temp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(pane: &str) -> Id {
        Id {
            host: "local".into(),
            session: "api".into(),
            tab: "main".into(),
            pane: pane.into(),
        }
    }

    fn file(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join("favorites.json")
    }

    fn loaded(path: &Path) -> Vec<Id> {
        Favorites::load(path).0.0
    }

    #[test]
    fn a_toggle_adds_and_removes_one_entry_and_a_missing_file_is_no_problem() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        assert_eq!(Favorites::load(&path), (Favorites::default(), None));
        set(&path, &id("seat-a"), true, &[], 1000).unwrap();
        set(&path, &id("seat-b"), true, &[], 1000).unwrap();
        set(&path, &id("seat-a"), true, &[], 1000).unwrap();
        assert_eq!(loaded(&path), [id("seat-a"), id("seat-b")]);
        set(&path, &id("seat-a"), false, &[], 1001).unwrap();
        assert_eq!(loaded(&path), [id("seat-b")]);
        set(&path, &id("seat-a"), false, &[], 1002).unwrap();
        assert_eq!(
            loaded(&path),
            [id("seat-b")],
            "off for one that is not there changes nothing"
        );
    }

    #[test]
    fn a_toggle_applies_only_itself_to_what_the_file_holds_now() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        set(&path, &id("seat-a"), true, &[], 1000).unwrap();
        // Another writer adds an entry and a key this version does not know between two toggles.
        let mut text: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        text["favorites"].as_array_mut().unwrap().push(
            serde_json::json!({"host": "mini", "session": "web", "tab": "main", "pane": "seat-c", "seen": 1000}),
        );
        text["future"] = serde_json::json!({"kept": true});
        fs::write(&path, text.to_string()).unwrap();
        set(&path, &id("seat-b"), true, &[], 1001).unwrap();
        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["favorites"].as_array().unwrap().len(), 3, "{saved}");
        assert_eq!(saved["future"], serde_json::json!({"kept": true}));
    }

    #[test]
    fn entries_unseen_for_30_days_go_at_the_next_save_and_live_ones_are_kept_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        let day = 24 * 60 * 60;
        set(&path, &id("gone"), true, &[], 0).unwrap();
        set(&path, &id("here"), true, &[], 0).unwrap();
        set(&path, &id("new"), true, &[id("here")], 29 * day).unwrap();
        assert_eq!(
            loaded(&path).len(),
            3,
            "29 days: all kept, dormant ones too"
        );
        set(&path, &id("new"), true, &[id("here")], 31 * day).unwrap();
        assert_eq!(
            loaded(&path),
            [id("here"), id("new")],
            "gone was unseen for 31 days"
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_means_none_one_reason_and_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        for bad in [
            "not json",
            "[]",
            r#"{"favorites": 3}"#,
            r#"{"favorites": [{"host": 1}]}"#,
        ] {
            fs::write(&path, bad).unwrap();
            let (none, why) = Favorites::load(&path);
            assert_eq!(none, Favorites::default());
            assert!(
                why.is_some_and(|why| why.contains("favorites.json")),
                "{bad}"
            );
            let err = set(&path, &id("seat-a"), true, &[], 1).unwrap_err();
            assert!(err.contains("move it aside"), "{err}");
            assert_eq!(fs::read_to_string(&path).unwrap(), bad, "left as it was");
        }
    }

    #[test]
    fn a_failed_save_is_an_error_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        set(&path, &id("seat-a"), true, &[], 1).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let err = set(&path, &id("seat-b"), true, &[], 2);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let err = err.unwrap_err();
        assert!(err.contains("favorites.json"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "the old list stands"
        );
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no temp file left"
        );
    }

    fn pane(host: &str, tab: &str, label: &str, block: &str) -> Row {
        serde_json::from_value(serde_json::json!({
            "key": format!("{host}/{block}"), "host": host, "session_id": "session:api",
            "session": "api", "window_id": "window:main", "tab": tab, "block_id": block,
            "pane": label, "title": "t", "harness": "claude-code", "state": "idle",
            "kind": null, "msg": null, "updated_at": "2026-01-01T00:00:00Z",
            "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
            "since_estimated": false, "turn_started": null, "last_turn": null,
            "seen": true, "stale": false, "suspect": false, "suspect_reason": null,
            "group": "quiet"
        }))
        .unwrap()
    }

    #[test]
    fn labels_are_the_identity_a_new_block_id_keeps_it_and_every_duplicate_is_marked() {
        let favorites = Favorites(vec![id("seat-a")]);
        let mut rows = vec![
            pane("local", "main", "seat-a", "block:1"),
            pane("local", "main", "seat-a", "block:2"),
            pane("local", "main", "seat-b", "block:3"),
            pane("local", "other", "seat-a", "block:4"),
            pane("mini", "main", "seat-a", "block:5"),
        ];
        favorites.apply(&mut rows);
        let marked: Vec<_> = rows.iter().map(|r| r.marked).collect();
        assert_eq!(marked, [true, true, false, false, false]);
        assert_eq!(rows[0].marked_by, ["favorite"]);
        // Rex gave the pane a new block id: the labels are the same, so it is still one.
        let mut again = vec![pane("local", "main", "seat-a", "block:99")];
        favorites.apply(&mut again);
        assert!(again[0].marked);
    }

    #[test]
    fn favorite_comes_last_among_the_sources_once_and_a_pane_with_no_label_has_none() {
        let favorites = Favorites(vec![id("seat-a"), id("")]);
        let mut rows = vec![
            pane("local", "main", "seat-a", "b"),
            pane("local", "main", "", "c"),
        ];
        rows[0].marked_by = vec!["config".into(), "team".into()];
        rows[0].marked = true;
        favorites.apply(&mut rows);
        favorites.apply(&mut rows);
        assert_eq!(rows[0].marked_by, ["config", "team", "favorite"]);
        assert!(
            !rows[1].marked,
            "nothing names a pane with no label of its own"
        );
        assert_eq!(Id::of(&rows[1]), None);
    }

    #[test]
    fn an_entry_no_pane_matches_is_dormant_and_comes_back_with_its_labels() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        set(&path, &id("seat-a"), true, &[], 1000).unwrap();
        let mut rows = vec![pane("local", "main", "renamed", "b")];
        Favorites::load(&path).0.apply(&mut rows);
        assert!(!rows[0].marked, "renamed: not shown");
        assert_eq!(loaded(&path), [id("seat-a")], "but kept");
        set(&path, &id("seat-b"), true, &[], 1001).unwrap();
        assert_eq!(
            loaded(&path),
            [id("seat-a"), id("seat-b")],
            "a save keeps it"
        );
        let mut back = vec![pane("local", "main", "seat-a", "b2")];
        Favorites::load(&path).0.apply(&mut back);
        assert!(back[0].marked, "the labels returned");
    }
}
