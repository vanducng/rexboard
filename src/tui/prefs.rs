//! tui.json: the board's own settings on this machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// `c` hid the card; missing is shown. The name is from when the card was a side pane, kept
    /// so an existing choice carries over.
    #[serde(default)]
    pub detail_hidden: bool,
}

/// A missing or unreadable tui.json is the defaults. Keys this build does not know, such as the
/// side pane's old `detail` width, are ignored.
pub fn load(path: &Path) -> Prefs {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Prefs>(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, prefs: &Prefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        path,
        serde_json::to_vec(prefs).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip_and_bad_files_give_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rexboard/tui.json");
        assert_eq!(load(&path), Prefs::default(), "missing");
        let kept = Prefs {
            detail_hidden: true,
        };
        save(&path, &kept).unwrap();
        assert_eq!(load(&path), kept);
        std::fs::write(&path, r#"{"detail": 0.4, "detail_hidden": true}"#).unwrap();
        assert!(
            load(&path).detail_hidden,
            "a file with the old width keeps its choice"
        );
        std::fs::write(&path, r#"{"detail": 0.4}"#).unwrap();
        assert!(!load(&path).detail_hidden, "a file from before c shows it");
        for bad in ["{", "[]", r#"{"detail_hidden": "yes"}"#] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load(&path), Prefs::default(), "{bad}");
        }
    }
}
