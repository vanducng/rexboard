//! tui.json: the board's own settings on this machine.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The opt-in columns to show, by name (`cost`, `5h`), kept as written so a save keeps a
    /// name a later build knows.
    #[serde(
        default,
        deserialize_with = "names",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub columns: Vec<String>,
    /// Fixed widths by column name (`host`, `session`, `window`, `block`, `msg`), kept as
    /// written; a name or a value this build does not know is ignored when the list is drawn.
    #[serde(
        default,
        deserialize_with = "widths",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub widths: BTreeMap<String, u16>,
}

/// Anything but an object is none; an entry whose value is not a width from 1 to 500 is dropped.
fn widths<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, u16>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Object(entries) => entries
            .into_iter()
            .filter_map(|(name, width)| {
                let width = u16::try_from(width.as_u64()?).ok()?;
                (1..=500).contains(&width).then_some((name, width))
            })
            .collect(),
        _ => BTreeMap::new(),
    })
}

/// Anything but a list of names is none, and keeps the rest of the file.
fn names<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    })
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
    fn widths_keep_every_whole_number_width_and_drop_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tui.json");
        std::fs::write(
            &path,
            r#"{"widths": {"host": 12, "msg": 60, "later": 7, "zero": 0, "neg": -3, "text": "9", "huge": 9999, "frac": 2.5}}"#,
        )
        .unwrap();
        assert_eq!(
            load(&path).widths,
            BTreeMap::from([
                ("host".to_owned(), 12),
                ("later".to_owned(), 7),
                ("msg".to_owned(), 60)
            ]),
            "a name this build does not know is kept as written; a value that is no width is dropped"
        );
        std::fs::write(&path, r#"{"widths": [1, 2], "detail_hidden": true}"#).unwrap();
        assert!(load(&path).widths.is_empty() && load(&path).detail_hidden);
    }

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
            columns: vec!["cost".into(), "5h".into()],
            widths: BTreeMap::from([("host".to_owned(), 12), ("whatever".to_owned(), 9)]),
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
                columns: Vec::new(),
                widths: BTreeMap::new(),
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
                columns: Vec::new(),
                widths: BTreeMap::new(),
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
        for (text, want) in [
            (r#"["cost", 5, "later"]"#, vec!["cost", "later"]),
            (r#""cost""#, vec![]),
        ] {
            std::fs::write(
                &path,
                format!(r#"{{"columns": {text}, "motion": "reduced"}}"#),
            )
            .unwrap();
            let prefs = load(&path);
            assert_eq!(prefs.columns, want, "{text}");
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
