use chrono::{DateTime, Utc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::board::Row;
use crate::pane::bare_title;

/// The widest a `session`, `tab` or `pane` label gets in `session › tab › pane`.
const SEGMENT_MAX: usize = 24;

/// The narrowest WHAT gets before WHERE gives up columns too.
const WHAT_MIN: usize = 16;

/// The terminal's width when stdout is one; piped output keeps every column whole.
pub fn terminal_width() -> Option<usize> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return None;
    }
    ratatui::crossterm::terminal::size()
        .ok()
        .map(|(cols, _)| usize::from(cols))
}

/// The `ls` table. Given a width, WHAT and then WHERE are cut so a line fits it.
pub fn table(rows: &[Row], now: DateTime<Utc>, width: Option<usize>) -> String {
    let mut cells: Vec<[String; 7]> = rows
        .iter()
        .map(|row| {
            let pane = &row.pane;
            [
                serde_json::to_value(row.group)
                    .ok()
                    .and_then(|group| group.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                pane.state.clone(),
                in_state(row, now),
                pane.harness.clone().unwrap_or_else(|| "-".into()),
                pane.host.clone(),
                place(row),
                what(row).to_owned(),
            ]
        })
        .collect();
    let header = ["GROUP", "STATE", "AGE", "HARNESS", "HOST", "WHERE", "WHAT"].map(String::from);
    let mut widths = [0; 7];
    for row in std::iter::once(&header).chain(&cells) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.width());
        }
    }
    if let Some(max) = width {
        let total = |w: &[usize; 7]| w.iter().sum::<usize>() + 2 * 6;
        let over = total(&widths).saturating_sub(max);
        let what = widths[6].saturating_sub(over).max(WHAT_MIN.min(widths[6]));
        let over = over.saturating_sub(widths[6] - what);
        let place = widths[5].saturating_sub(over).max("WHERE".len());
        widths[5] = place;
        widths[6] = what;
        for row in &mut cells {
            row[5] = squeeze(&row[5], place);
            row[6] = fit(&row[6], what).trim_end().to_owned();
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&header).chain(&cells) {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            line.push_str(cell);
            if i + 1 < row.len() {
                line.push_str(&" ".repeat(widths[i].saturating_sub(cell.width()) + 2));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Time in the current state: `-` with no status record to date it, `≥` when estimated.
pub fn in_state(row: &Row, now: DateTime<Utc>) -> String {
    if row.pane.source == "inferred" {
        return "-".into();
    }
    let estimated = if row.since_estimated { "≥" } else { "" };
    format!("{estimated}{}", age(&row.since, now))
}

/// The record's message, else the title without its state glyph.
pub fn what(row: &Row) -> &str {
    row.pane
        .msg
        .as_deref()
        .filter(|msg| !msg.is_empty())
        .unwrap_or_else(|| bare_title(&row.pane.title))
}

/// `session › tab › pane`, each label capped so one long label cannot swamp the column.
pub fn place(row: &Row) -> String {
    let p = &row.pane;
    [&p.session, &p.tab, &p.pane]
        .map(|label| squeeze(label, SEGMENT_MAX))
        .join(" › ")
}

/// `text` in at most `max` columns, with an ellipsis in the middle when it does not fit.
pub fn squeeze(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let room = max.saturating_sub(1);
    let head = take(text.chars(), room.div_ceil(2));
    let tail: String = take(text.chars().rev(), room / 2).chars().rev().collect();
    format!("{head}…{tail}")
}

/// `text` in at most `max` columns, cut at the end with an ellipsis, then padded to `max`.
pub fn fit(text: &str, max: usize) -> String {
    let cut = if text.width() <= max {
        text.to_owned()
    } else {
        format!("{}…", take(text.chars(), max.saturating_sub(1)))
    };
    let pad = max.saturating_sub(cut.width());
    format!("{cut}{}", " ".repeat(pad))
}

/// The longest prefix of `chars` that fits in `max` columns.
fn take(chars: impl Iterator<Item = char>, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in chars {
        let w = c.width().unwrap_or(0);
        if used + w > max {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

pub fn age(at: &str, now: DateTime<Utc>) -> String {
    let Ok(at) = DateTime::parse_from_rfc3339(at) else {
        return "-".into();
    };
    let secs = (now - at.with_timezone(&Utc)).num_seconds().max(0);
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h{}m", secs / 3600, secs % 3600 / 60),
        _ => format!("{}d{}h", secs / 86400, secs % 86400 / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_shows_two_units_past_an_hour() {
        let now = DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(age("2026-01-01T23:59:30Z", now), "30s");
        assert_eq!(age("2026-01-01T23:48:00+00:00", now), "12m");
        assert_eq!(age("2026-01-02T04:00:00+07:00", now), "3h0m");
        assert_eq!(age("2026-01-01T18:17:00Z", now), "5h43m");
        assert_eq!(age("2025-12-30T00:00:00Z", now), "3d0h");
        assert_eq!(age("not a time", now), "-");
    }

    #[test]
    fn table_fits_a_given_width_by_cutting_what_then_where() {
        let row: Row = serde_json::from_value(serde_json::json!({
            "key": "mini/b", "host": "mini", "session_id": "s", "session": "web", "window_id": "w",
            "tab": "2-openrig:rig:demo-team#0123456789abcdef#l2", "block_id": "b",
            "pane": "review.review", "title": "", "harness": "claude-code", "state": "working",
            "kind": null, "msg": "Reviewing the parser change across every module", "updated_at": null,
            "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
            "since_estimated": false, "turn_started": null, "last_turn": null, "seen": true,
            "stale": false, "group": "working"
        }))
        .unwrap();
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:34:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let full = table(std::slice::from_ref(&row), now, None);
        assert!(full.contains("Reviewing the parser change across every module"));
        for width in [80, 100] {
            let text = table(std::slice::from_ref(&row), now, Some(width));
            for line in text.lines() {
                assert!(line.width() <= width, "{} > {width}: {line}", line.width());
            }
            assert!(text.contains("…"), "{text}");
        }
    }

    #[test]
    fn squeeze_keeps_both_ends_of_a_long_label() {
        let tab = "2-openrig:rig:demo-team#0123456789abcdef#l2";
        let short = squeeze(tab, 24);
        assert_eq!(short, "2-openrig:ri…89abcdef#l2");
        assert!(short.width() <= 24);
        assert_eq!(squeeze("main", 24), "main");
    }

    #[test]
    fn widths_count_wide_characters_as_two_columns() {
        let wide = "修复解析器🙂 tests";
        let short = squeeze(wide, 9);
        assert!(short.width() <= 9, "{short} is {} columns", short.width());
        assert_eq!(short, "修复…ests");
        let cell = fit(wide, 8);
        assert_eq!(cell.width(), 8, "{cell:?}");
        assert_eq!(cell, "修复解… ");
        assert_eq!(fit("ok", 4), "ok  ");
    }
}
