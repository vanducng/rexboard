//! tui.json: the board's own settings on this machine.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// `c` hid the card; missing is shown. The name is from when the card was a side pane, kept
    /// so an existing choice carries over.
    #[serde(default)]
    pub detail_hidden: bool,
    #[serde(default)]
    pub motion: Motion,
    #[serde(default)]
    pub icons: Icons,
    #[serde(
        default,
        deserialize_with = "card_height",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_height: Option<CardHeight>,
}

/// `"card_height": 12` rows or `"card_height": "40%"` of the board, the same for every row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardHeight {
    Rows(u16),
    Percent(u16),
}

impl Serialize for CardHeight {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            CardHeight::Rows(rows) => s.serialize_u16(*rows),
            CardHeight::Percent(percent) => s.serialize_str(&format!("{percent}%")),
        }
    }
}

/// Anything else is the default height, and keeps the rest of the file.
fn card_height<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CardHeight>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(rows) => rows
            .as_u64()
            .and_then(|rows| u16::try_from(rows).ok())
            .filter(|rows| *rows > 0)
            .map(CardHeight::Rows),
        serde_json::Value::String(text) => text
            .strip_suffix('%')
            .and_then(|percent| percent.trim().parse().ok())
            .filter(|percent| (1..=100).contains(percent))
            .map(CardHeight::Percent),
        _ => None,
    })
}

/// `"motion": "reduced"` holds the working spinner still.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    Reduced,
    /// Any other value too, so a typo keeps the rest of the file.
    #[default]
    #[serde(other)]
    Full,
}

/// `"icons": "nerd"` labels the card's facts with Nerd Font glyphs; `"text"`, the default, with
/// words.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    Nerd,
    /// Any other value too, so a typo keeps the rest of the file.
    #[default]
    #[serde(other)]
    Text,
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
            motion: Motion::Reduced,
            icons: Icons::Nerd,
            card_height: Some(CardHeight::Percent(30)),
        };
        save(&path, &kept).unwrap();
        assert_eq!(load(&path), kept);
        std::fs::write(&path, r#"{"detail": 0.4, "detail_hidden": true}"#).unwrap();
        assert!(
            load(&path).detail_hidden,
            "a file with the old width keeps its choice"
        );
        std::fs::write(&path, r#"{"detail_hidden": true, "motion": "still"}"#).unwrap();
        assert_eq!(
            load(&path),
            Prefs {
                detail_hidden: true,
                motion: Motion::Full,
                icons: Icons::Text,
                card_height: None,
            },
            "an unknown motion is full and keeps the rest"
        );
        std::fs::write(&path, r#"{"icons": "emoji", "motion": "reduced"}"#).unwrap();
        assert_eq!(
            load(&path),
            Prefs {
                detail_hidden: false,
                motion: Motion::Reduced,
                icons: Icons::Text,
                card_height: None,
            },
            "unknown icons are words and keep the rest"
        );
        for (text, want) in [
            ("12", Some(CardHeight::Rows(12))),
            (r#""25%""#, Some(CardHeight::Percent(25))),
            ("0", None),
            (r#""150%""#, None),
            (r#""tall""#, None),
            ("true", None),
        ] {
            std::fs::write(
                &path,
                format!(r#"{{"card_height": {text}, "motion": "reduced"}}"#),
            )
            .unwrap();
            let prefs = load(&path);
            assert_eq!(prefs.card_height, want, "{text}");
            assert_eq!(prefs.motion, Motion::Reduced, "{text} keeps the rest");
        }
        std::fs::write(&path, r#"{"detail": 0.4}"#).unwrap();
        assert!(!load(&path).detail_hidden, "a file from before c shows it");
        for bad in ["{", "[]", r#"{"detail_hidden": "yes"}"#] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load(&path), Prefs::default(), "{bad}");
        }
    }
}
