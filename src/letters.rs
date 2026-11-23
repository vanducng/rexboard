//! Letter marks: a pane holds a letter from a to z, kept in letters.json beside favorites.json.
//!
//! As for favorites, a pane is known by its labels, never by a block id, so a letter survives a
//! Rex restart. A letter is held by one pane, and a pane holds one letter.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::board::Row;
use crate::favorites::{Id, save};

/// What a row's `marked_by` says when its pane holds a letter.
pub const LETTER: &str = "letter";

/// An entry whose pane has not been on the board for this long goes at the next save.
const KEEP_SECS: i64 = 30 * 24 * 60 * 60;

/// A live entry is written again once it is this much older than now.
const REFRESH_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    letter: char,
    #[serde(flatten)]
    id: Id,
    /// Unix seconds when a save last found its pane on the board, or when it was set.
    seen: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    letters: Vec<Entry>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

fn read(path: &Path) -> Result<File, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(err) => return Err(format!("{}: {err}", path.display())),
    };
    serde_json::from_str(&text)
        .map_err(|err| format!("{}: not a letters file: {err}", path.display()))
}

/// The letters in force, in letter order.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Letters(Vec<(char, Id)>);

impl Letters {
    /// What the file holds; a file that cannot be read gives none and says why.
    pub fn load(path: &Path) -> (Letters, Option<String>) {
        match read(path) {
            Ok(file) => {
                let mut held: Vec<_> = file.letters.into_iter().map(|e| (e.letter, e.id)).collect();
                held.sort_by_key(|(letter, _)| *letter);
                (Letters(held), None)
            }
            Err(why) => (Letters::default(), Some(why)),
        }
    }

    pub fn held(&self) -> &[(char, Id)] {
        &self.0
    }

    pub fn pane(&self, letter: char) -> Option<&Id> {
        self.0.iter().find(|(l, _)| *l == letter).map(|(_, id)| id)
    }

    pub fn of(&self, id: &Id) -> Option<char> {
        self.0.iter().find(|(_, held)| held == id).map(|(l, _)| *l)
    }

    /// Gives each row whose pane holds a letter that letter and `letter` as a source, last among
    /// the sources. Every pane with the same labels in a tab shows it.
    pub fn apply(&self, rows: &mut [Row]) {
        for row in rows {
            row.letter = Id::of(row).and_then(|id| self.of(&id));
            if row.letter.is_some() {
                row.marked_by.retain(|source| source != LETTER);
                row.marked_by.push(LETTER.into());
                row.marked = true;
            }
        }
    }
}

/// Whether `text` names a letter from a to z.
pub fn letter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    chars
        .next()
        .filter(|c| c.is_ascii_lowercase() && chars.next().is_none())
}

/// Gives `id` the letter, taking it from the pane that held it; says who held it. The file is
/// read again first and only this change is made; `live` are the panes on the board now. A file
/// that cannot be read is left alone: it is the user's.
pub fn set(
    path: &Path,
    letter: char,
    id: &Id,
    live: &[Id],
    now: i64,
) -> Result<Option<Id>, String> {
    let mut file = read(path).map_err(|why| format!("{why}; move it aside to start anew"))?;
    see(&mut file, live, now, 0);
    let before = file
        .letters
        .iter()
        .find(|e| e.letter == letter)
        .map(|e| e.id.clone());
    file.letters.retain(|e| e.letter != letter && e.id != *id);
    file.letters.push(Entry {
        letter,
        id: id.clone(),
        seen: now,
    });
    finish(path, file, now)?;
    Ok(before.filter(|held| held != id))
}

/// Frees `letter`; says which pane held it.
pub fn clear(path: &Path, letter: char, live: &[Id], now: i64) -> Result<Option<Id>, String> {
    let mut file = read(path).map_err(|why| format!("{why}; move it aside to start anew"))?;
    see(&mut file, live, now, 0);
    let held = file
        .letters
        .iter()
        .find(|e| e.letter == letter)
        .map(|e| e.id.clone());
    file.letters.retain(|e| e.letter != letter);
    finish(path, file, now)?;
    Ok(held)
}

fn finish(path: &Path, mut file: File, now: i64) -> Result<(), String> {
    file.letters.retain(|entry| now - entry.seen <= KEEP_SECS);
    file.letters.sort_by_key(|e| e.letter);
    save(path, &file).map_err(|err| format!("{}: {err}", path.display()))
}

fn see(file: &mut File, live: &[Id], now: i64, older: i64) -> bool {
    let mut changed = false;
    for entry in &mut file.letters {
        if live.contains(&entry.id) && now - entry.seen >= older && entry.seen != now {
            entry.seen = now;
            changed = true;
        }
    }
    changed
}

/// Keeps the entries of the panes on the board now from aging out, writing only when one was last
/// seen over a day ago.
pub fn touch(path: &Path, live: &[Id], now: i64) -> Result<(), String> {
    let Ok(mut file) = read(path) else {
        return Ok(());
    };
    if !see(&mut file, live, now, REFRESH_SECS) {
        return Ok(());
    }
    finish(path, file, now)
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

    fn held(path: &Path) -> Vec<(char, String)> {
        Letters::load(path)
            .0
            .held()
            .iter()
            .map(|(l, id)| (*l, id.pane.clone()))
            .collect()
    }

    #[test]
    fn only_a_single_lower_case_letter_is_a_letter() {
        assert_eq!(letter("a"), Some('a'));
        for bad in ["", "A", "ab", "1", "-", "é"] {
            assert_eq!(letter(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_letter_moves_to_the_new_pane_and_a_pane_holds_one_letter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("letters.json");
        assert_eq!(set(&path, 'b', &id("one"), &[], 10).unwrap(), None);
        assert_eq!(set(&path, 'a', &id("two"), &[], 10).unwrap(), None);
        assert_eq!(
            set(&path, 'b', &id("two"), &[], 11).unwrap(),
            Some(id("one")),
            "b moved from one to two"
        );
        assert_eq!(
            held(&path),
            [('b', "two".to_owned())],
            "two's a was replaced"
        );
        assert_eq!(clear(&path, 'b', &[], 12).unwrap(), Some(id("two")));
        assert_eq!(clear(&path, 'b', &[], 12).unwrap(), None);
        assert!(held(&path).is_empty());
    }

    #[test]
    fn an_entry_unseen_for_30_days_goes_and_a_file_that_cannot_be_read_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("letters.json");
        let day = 24 * 60 * 60;
        set(&path, 'a', &id("gone"), &[], 0).unwrap();
        set(&path, 'b', &id("here"), &[], 0).unwrap();
        set(&path, 'c', &id("new"), &[id("here")], 31 * day).unwrap();
        assert_eq!(
            held(&path),
            [('b', "here".to_owned()), ('c', "new".to_owned())]
        );
        fs::write(&path, "not json").unwrap();
        let err = set(&path, 'a', &id("x"), &[], 1).unwrap_err();
        assert!(err.contains("move it aside"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
        assert!(Letters::load(&path).1.is_some());
    }
}
