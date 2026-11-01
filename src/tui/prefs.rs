//! tui.json: the board's own settings on this machine.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// `"card": "on"`: `c` has the card showing. Missing is off; another value is missing too.
    #[serde(
        default,
        deserialize_with = "switch",
        serialize_with = "switch_out",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<bool>,
    #[serde(flatten)]
    pub tuning: Tuning,
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
    /// written; the list checks a name and a value only when it applies them.
    #[serde(
        default,
        deserialize_with = "widths",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub widths: BTreeMap<String, serde_json::Value>,
}

/// The settings the board only reads, kept as written so a save carries them: `detail_hidden`
/// is from when the card was shown by default and counts only while `card` is missing.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tuning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_hidden: Option<bool>,
    /// `preview`, `preview_lines` and `preview_every` belonged to the retired preview strip: they
    /// are not read, and a save writes them back as they were.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub preview: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub preview_lines: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub preview_every: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub tail_lines: Option<serde_json::Value>,
}

impl Tuning {
    /// The tail the card and the full view keep, 50-5000 lines; anything else is 200.
    pub fn tail(&self) -> usize {
        within(&self.tail_lines, 50..=5000, DEFAULT_TAIL as u64) as usize
    }
}

/// The tail a peek keeps by default.
pub const DEFAULT_TAIL: usize = 200;

fn within(
    n: &Option<serde_json::Value>,
    range: std::ops::RangeInclusive<u64>,
    default: u64,
) -> u64 {
    n.as_ref()
        .and_then(serde_json::Value::as_u64)
        .filter(|n| range.contains(n))
        .unwrap_or(default)
}

impl Prefs {
    /// Whether the card shows: `card`, else the old `detail_hidden` turned round, else off.
    pub fn card_on(&self) -> bool {
        self.card
            .or(self.tuning.detail_hidden.map(|hidden| !hidden))
            .unwrap_or(false)
    }
}

/// Whatever is written, an explicit `null` too: only a missing key is none.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(d).map(Some)
}

/// `"on"` or `"off"`; anything else is missing.
fn switch<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)?.as_str() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        _ => None,
    })
}

fn switch_out<S: Serializer>(on: &Option<bool>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(if on.unwrap_or(false) { "on" } else { "off" })
}

/// Anything but an object is none.
fn widths<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, serde_json::Value>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Object(entries) => entries.into_iter().collect(),
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
    fn widths_are_kept_as_written_and_checked_only_when_applied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tui.json");
        let written = r#"{"widths": {"host": 12, "session": 0, "window": "auto", "future": 501, "flag": true}}"#;
        std::fs::write(&path, written).unwrap();
        let prefs = load(&path);
        assert_eq!(prefs.widths.len(), 5, "every entry is kept");
        save(&path, &prefs).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            back["widths"],
            serde_json::from_str::<serde_json::Value>(written).unwrap()["widths"],
            "a save writes back what was read, as the card toggle does"
        );
        std::fs::write(&path, r#"{"widths": [1, 2], "detail_hidden": true}"#).unwrap();
        assert!(load(&path).widths.is_empty() && load(&path).tuning.detail_hidden == Some(true));
    }

    #[test]
    fn prefs_round_trip_and_bad_files_give_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rexboard/tui.json");
        assert_eq!(load(&path), Prefs::default(), "missing");
        let kept = Prefs {
            card: Some(true),
            tuning: Tuning {
                detail_hidden: Some(true),
                preview: Some("on".into()),
                preview_lines: Some(6.into()),
                preview_every: Some(3.into()),
                tail_lines: Some(1000.into()),
            },
            motion: Motion::Reduced,
            icons: Icons::Nerd,
            card_height: Some(CardHeight::Percent(30)),
            columns: vec!["cost".into(), "5h".into()],
            widths: BTreeMap::from([
                ("host".to_owned(), 12.into()),
                ("whatever".to_owned(), 9.into()),
            ]),
        };
        save(&path, &kept).unwrap();
        assert_eq!(load(&path), kept);
        std::fs::write(&path, r#"{"detail": 0.4, "detail_hidden": true}"#).unwrap();
        assert_eq!(
            load(&path).tuning.detail_hidden,
            Some(true),
            "a file with the old width keeps its choice"
        );
        std::fs::write(&path, r#"{"detail_hidden": true, "motion": "still"}"#).unwrap();
        assert_eq!(
            load(&path),
            Prefs {
                tuning: Tuning {
                    detail_hidden: Some(true),
                    ..Tuning::default()
                },
                motion: Motion::Full,
                icons: Icons::Text,
                ..Prefs::default()
            },
            "an unknown motion is full and keeps the rest"
        );
        std::fs::write(&path, r#"{"icons": "emoji", "motion": "reduced"}"#).unwrap();
        assert_eq!(
            load(&path),
            Prefs {
                motion: Motion::Reduced,
                ..Prefs::default()
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
        assert!(
            !load(&path).card_on(),
            "a file with neither key has the card off"
        );
        for bad in ["{", "[]", r#"{"detail_hidden": "yes"}"#] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load(&path), Prefs::default(), "{bad}");
        }
    }

    #[test]
    fn the_old_detail_hidden_is_read_for_the_card_and_kept_on_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tui.json");
        for (text, card) in [
            ("{}", false),
            (r#"{"detail_hidden": true}"#, false),
            (r#"{"detail_hidden": false}"#, true),
            (r#"{"card": "off", "detail_hidden": false}"#, false),
            (r#"{"card": "on", "detail_hidden": true}"#, true),
            (r#"{"card": "maybe", "detail_hidden": false}"#, true),
            (r#"{"card": true}"#, false),
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(load(&path).card_on(), card, "{text}");
        }
        std::fs::write(&path, r#"{"detail_hidden": false, "motion": "reduced"}"#).unwrap();
        let mut prefs = load(&path);
        prefs.card = Some(false);
        save(&path, &prefs).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["card"], "off");
        assert_eq!(back["detail_hidden"], false, "kept as it was read");
        assert_eq!(back["motion"], "reduced");
        assert!(!load(&path).card_on(), "the card key wins from then on");
    }

    #[test]
    fn tail_lines_is_checked_when_used_and_the_retired_preview_keys_are_kept_as_written() {
        let tune = |text: &str| -> Tuning { serde_json::from_str::<Prefs>(text).unwrap().tuning };
        assert_eq!(tune("{}").tail(), 200);
        for (n, want) in [(49, 200), (50, 50), (5000, 5000), (5001, 200)] {
            assert_eq!(tune(&format!(r#"{{"tail_lines": {n}}}"#)).tail(), want);
        }
        assert_eq!(
            tune(r#"{"tail_lines": "1000"}"#).tail(),
            200,
            "not a number"
        );
        // Whatever was written comes back on a save, valid or not, the retired keys included.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tui.json");
        let written =
            r#"{"preview": "on", "preview_lines": -1, "preview_every": 1.5, "tail_lines": "many"}"#;
        std::fs::write(&path, written).unwrap();
        let mut prefs = load(&path);
        assert_eq!(prefs.tuning.tail(), 200);
        prefs.card = Some(true);
        save(&path, &prefs).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["preview"], "on");
        assert_eq!(back["preview_lines"], -1);
        assert_eq!(back["preview_every"], 1.5);
        assert_eq!(back["tail_lines"], "many");
        // A key written as null is still there after a save; a missing one is not added.
        std::fs::write(&path, r#"{"preview_lines": null, "tail_lines": null}"#).unwrap();
        let mut prefs = load(&path);
        prefs.card = Some(true);
        save(&path, &prefs).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(back["preview_lines"].is_null() && back.get("preview_lines").is_some());
        assert!(back.get("tail_lines").is_some());
        assert!(back.get("preview_every").is_none(), "missing stays missing");
        assert!(back.get("preview").is_none(), "missing stays missing");
    }
}
