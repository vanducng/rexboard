use chrono::{DateTime, Utc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::board::Row;
use crate::pane::{Pane, bare_title};

/// The widest a `session`, `tab` or `pane` label gets in `session › tab › pane`.
const SEGMENT_MAX: usize = 24;

/// The narrowest a session label is cut to, so its stem stays readable.
const SESSION_MIN: usize = 8;

/// A tab this narrow or less stays whole while there is anything else to cut.
const SHORT_TAB: usize = 6;

/// The narrowest WHAT gets before WHERE gives up columns too.
const WHAT_MIN: usize = 16;

const SEP: &str = " › ";

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

/// The `ls` table. Given a width, WHAT and then WHERE are cut so a line fits it. Every cell is
/// cleaned, since titles and messages come from programs in other panes.
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
                state_word(row),
                in_state(row, now),
                harness(row),
                pane.host.clone(),
                String::new(),
                what(row).to_owned(),
            ]
            .map(|cell| clean(&cell))
        })
        .collect();
    for (cell, place) in cells.iter_mut().zip(wheres(rows, usize::MAX, false, true)) {
        cell[5] = place;
    }
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
        for (cell, place) in cells.iter_mut().zip(wheres(rows, place, false, true)) {
            cell[5] = place;
            cell[6] = fit(&cell[6], what).trim_end().to_owned();
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

/// The short harness name the board and `ls` show: `claude` for claude-code, and a `?` when it
/// is read from the title because the pane has no status record.
pub fn harness(row: &Row) -> String {
    let name = match row.pane.harness.as_deref() {
        Some("claude-code") => "claude",
        Some(other) => other,
        None => "-",
    };
    if row.pane.source == "inferred" && row.pane.harness.is_some() {
        format!("{name}?")
    } else {
        name.to_owned()
    }
}

/// The state to show: `idle?` for a suspect `working` row, whose record is not believed.
pub fn state_word(row: &Row) -> String {
    if row.suspect {
        "idle?".to_owned()
    } else {
        row.pane.state.clone()
    }
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

/// A pane label that names nothing: `shell`, or what runs in the pane.
fn generic(pane: &Pane) -> bool {
    pane.pane == "shell" || pane.process.as_deref() == Some(pane.pane.as_str())
}

/// WHERE's labels: `session › tab › pane`, or `session › tab` when the pane label is generic.
/// `short` keeps two labels for narrow rows: `session › pane`, or `session › tab`.
pub fn labels(row: &Row, short: bool) -> Vec<&str> {
    let p = &row.pane;
    match (generic(p), short) {
        (true, _) => vec![&p.session, &p.tab],
        (false, false) => vec![&p.session, &p.tab, &p.pane],
        (false, true) => vec![&p.session, &p.pane],
    }
}

/// WHERE for each row in `max` columns. Rows that would still read the same get told apart:
/// `@HOST` for a host other than `local` when they are on different hosts and the host has no
/// column of its own, and `#N`, the pane's place among its tab's agent panes, when another of
/// them is on the same host. The suffix is never cut; the path gives way, down to `…`.
pub fn wheres(rows: &[Row], max: usize, short: bool, host_column: bool) -> Vec<String> {
    let first: Vec<String> = rows
        .iter()
        .map(|row| path(&labels(row, short), max))
        .collect();
    rows.iter()
        .zip(&first)
        .map(|(row, place)| {
            let twins: Vec<&Row> = rows
                .iter()
                .zip(&first)
                .filter(|(other, text)| {
                    *text == place && (!host_column || other.pane.host == row.pane.host)
                })
                .map(|(other, _)| other)
                .collect();
            let mut suffix = String::new();
            if row.pane.host != "local"
                && twins.iter().any(|other| other.pane.host != row.pane.host)
            {
                suffix.push_str(&format!(" @{}", clean(&row.pane.host)));
            }
            if twins
                .iter()
                .filter(|other| other.pane.host == row.pane.host)
                .count()
                > 1
            {
                suffix.push_str(&format!(" #{}", position(row, rows)));
            }
            if suffix.is_empty() {
                return place.clone();
            }
            let room = max.saturating_sub(suffix.width());
            let place = if room < 5 {
                "…".to_owned()
            } else {
                path(&labels(row, short), room)
            };
            format!("{place}{suffix}")
        })
        .collect()
}

/// The pane's place among the agent panes of its tab, oldest first: Rex block ids sort by
/// creation time.
fn position(row: &Row, rows: &[Row]) -> usize {
    let p = &row.pane;
    1 + rows
        .iter()
        .filter(|other| {
            let o = &other.pane;
            o.host == p.host && o.window_id == p.window_id && o.block_id < p.block_id
        })
        .count()
}

/// What to call a row: its pane label, or for a generic one its title, else its tab.
pub fn name(row: &Row) -> String {
    let p = &row.pane;
    let title = bare_title(&p.title);
    let name = if !generic(p) {
        &p.pane
    } else if title.is_empty() {
        &p.tab
    } else {
        title
    };
    clean(name)
}

/// Labels joined with ` › ` in at most `max` columns, with the separators kept. The cuts go to
/// the least telling label first: a long tab, then the session down to a readable stem, then
/// the last label; only past those do a short tab and the stems give way.
pub fn path(labels: &[&str], max: usize) -> String {
    let whole: Vec<String> = labels.iter().map(|label| clean(label)).collect();
    let mut parts: Vec<String> = whole
        .iter()
        .map(|label| squeeze(label, SEGMENT_MAX))
        .collect();
    let width = |parts: &[String]| {
        parts.iter().map(|p| p.width()).sum::<usize>() + SEP.width() * parts.len().saturating_sub(1)
    };
    let last = parts.len().saturating_sub(1);
    let middle: Vec<usize> = (1..last).collect();
    // Each pass: the labels it cuts, the narrowest it leaves them, and the width a label must
    // pass for this pass to touch it.
    let passes = [
        (middle.clone(), 0, SHORT_TAB),
        (vec![0], SESSION_MIN, 0),
        (vec![last], SHORT_TAB, 0),
        (vec![0], 3, 0),
        (middle.clone(), 0, 0),
        (vec![last], 1, 0),
    ];
    for (cut, floor, wider) in passes {
        for i in cut {
            let over = width(&parts).saturating_sub(max);
            let now = parts[i].width();
            if over == 0 || now <= floor.max(1) || now <= wider {
                continue;
            }
            let keep = now.saturating_sub(over).max(floor);
            // A tab cut to a few columns reads as noise; `…` alone says one is there.
            parts[i] = if middle.contains(&i) && keep <= SHORT_TAB {
                "…".to_owned()
            } else {
                squeeze(&whole[i], keep)
            };
        }
    }
    let joined = parts.join(SEP);
    if joined.width() <= max {
        joined
    } else {
        fit(&joined, max).trim_end().to_owned()
    }
}

/// `text` without terminal control: whole escape sequences (CSI, OSC ended by BEL or ST, and
/// their one-byte C1 forms) and any other control character. A tab becomes a space.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']' | 'P' | '_' | '^') => skip_string(&mut chars),
                _ => {}
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\u{9d}' | '\u{90}' | '\u{9f}' | '\u{9e}' => skip_string(&mut chars),
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Skips a CSI's parameters through its final byte.
fn skip_csi(chars: &mut impl Iterator<Item = char>) {
    for c in chars.by_ref() {
        if ('\x40'..='\x7e').contains(&c) {
            return;
        }
    }
}

/// Skips an OSC-like string through BEL, ESC `\` or the one-byte ST.
fn skip_string(chars: &mut std::iter::Peekable<impl Iterator<Item = char>>) {
    while let Some(c) = chars.next() {
        match c {
            '\x07' | '\u{9c}' => return,
            '\x1b' if chars.peek() == Some(&'\\') => {
                chars.next();
                return;
            }
            _ => {}
        }
    }
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

/// `text`, cleaned, in at most `max` columns, cut at the end with an ellipsis, then padded.
pub fn fit(text: &str, max: usize) -> String {
    let text = clean(text);
    let cut = if text.width() <= max {
        text
    } else {
        format!("{}…", take(text.chars(), max.saturating_sub(1)))
    };
    let pad = max.saturating_sub(cut.width());
    format!("{cut}{}", " ".repeat(pad))
}

/// `text` in rows of at most `max` columns, never splitting a wide character.
pub fn chop(text: &str, max: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > max && used > 0 {
            rows.push(String::new());
            used = 0;
        }
        used += w;
        rows.last_mut().expect("one row").push(c);
    }
    rows
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
    span((now - at.with_timezone(&Utc)).num_seconds())
}

/// A length of time, in the two largest units past an hour: `30s`, `12m`, `5h43m`, `3d0h`.
pub fn span(secs: i64) -> String {
    let secs = secs.max(0);
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
    fn chop_never_splits_a_wide_character() {
        assert_eq!(chop("ab修复c", 3), ["ab", "修", "复c"]);
        assert_eq!(chop("", 3), [""]);
    }

    fn row(pane: &str, title: &str, msg: &str) -> Row {
        serde_json::from_value(serde_json::json!({
            "key": "mini/b", "host": "mini", "session_id": "s", "session": "web", "window_id": "w",
            "tab": "2-openrig:rig:demo-team#0123456789abcdef#l2", "block_id": "b",
            "pane": pane, "title": title, "harness": "claude-code", "state": "working",
            "kind": null, "msg": msg, "updated_at": null,
            "source": "native", "children": {}, "since": "2026-01-01T00:00:00.000Z",
            "since_estimated": false, "turn_started": null, "last_turn": null, "seen": true,
            "stale": false, "suspect": false, "suspect_reason": null, "group": "working"
        }))
        .unwrap()
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:34:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn age_shows_two_units_past_an_hour() {
        let now = DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(age("2026-01-01T23:59:30Z", now), "30s");
        assert_eq!(age("2026-01-01T23:48:00+00:00", now), "12m");
        assert_eq!(age("2026-01-01T17:00:00-03:00", now), "4h0m");
        assert_eq!(age("2026-01-01T18:17:00Z", now), "5h43m");
        assert_eq!(age("2025-12-30T00:00:00Z", now), "3d0h");
        assert_eq!(age("not a time", now), "-");
    }

    #[test]
    fn table_fits_a_given_width_by_cutting_what_then_where() {
        let row = row(
            "seat-r",
            "",
            "Reviewing the parser change across every module",
        );
        let full = table(std::slice::from_ref(&row), now(), None);
        assert!(full.contains("Reviewing the parser change across every module"));
        assert!(
            full.contains(" claude "),
            "ls uses the board's short names: {full}"
        );
        for width in [80, 100] {
            let text = table(std::slice::from_ref(&row), now(), Some(width));
            for line in text.lines() {
                assert!(line.width() <= width, "{} > {width}: {line}", line.width());
            }
            assert!(text.contains(" › seat-r"), "the pane stays whole: {text}");
        }
    }

    #[test]
    fn path_cuts_the_tab_then_the_session_and_keeps_separators() {
        let labels = [
            "api",
            "2-openrig:rig:demo-team#0123456789abcdef#l2",
            "seat-d",
        ];
        assert_eq!(path(&labels, 60), "api › 2-openrig:ri…89abcdef#l2 › seat-d");
        assert_eq!(path(&labels, 30), "api › 2-openr…cdef#l2 › seat-d");
        assert_eq!(path(&labels, 16), "api › … › seat-d");
        assert_eq!(
            path(&labels, 14),
            "api › … › se…d",
            "the session keeps its stem"
        );
        assert_eq!(path(&["web", "shell"], 9), "web › s…l");
        let gateway = [
            "api-gateway",
            "2-openrig:rig:demo-team#0123456789abcdef#l2",
            "seat-r",
        ];
        assert_eq!(path(&gateway, 25), "api-gateway › … › seat-r");
        assert_eq!(
            path(&gateway, 20),
            "api…way › … › seat-r",
            "never a bare session"
        );
        assert_eq!(
            path(&labels, 20),
            "api › … › seat-d",
            "a tab cut below 6 columns is `…` alone"
        );
    }

    #[test]
    fn a_generic_pane_label_gives_way_to_the_tab() {
        let mut shell = row("shell", "", "Allow Bash?");
        (shell.pane.session, shell.pane.tab) = ("api".into(), "build".into());
        let mut codex = shell.clone();
        (codex.pane.block_id, codex.pane.pane, codex.pane.title) =
            ("c".into(), "codex".into(), "Fix the parser".into());
        codex.pane.process = Some("codex".into());
        codex.pane.window_id = "other".into();
        assert_eq!(labels(&shell, false), ["api", "build"]);
        assert_eq!(labels(&shell, true), ["api", "build"]);
        assert_eq!(name(&shell), "build", "no title: the tab");
        assert_eq!(
            labels(&codex, false),
            ["api", "build"],
            "named after its process"
        );
        assert_eq!(
            name(&codex),
            "Fix the parser",
            "the title when there is one"
        );
        let mut seat = shell.clone();
        (seat.pane.block_id, seat.pane.pane) = ("s".into(), "seat-a".into());
        assert_eq!(labels(&seat, false), ["api", "build", "seat-a"]);
        assert_eq!(labels(&seat, true), ["api", "seat-a"]);
        assert_eq!(name(&seat), "seat-a");
    }

    #[test]
    fn rows_that_read_the_same_get_their_place_in_the_tab_or_their_host() {
        let mut first = row("shell", "", "");
        (first.pane.session, first.pane.tab, first.pane.block_id) =
            ("api".into(), "build".into(), "block:01".into());
        let mut second = first.clone();
        second.pane.block_id = "block:02".into();
        let mut seat = first.clone();
        (seat.pane.block_id, seat.pane.pane) = ("block:03".into(), "seat-a".into());
        let rows = vec![second.clone(), first.clone(), seat];
        assert_eq!(
            wheres(&rows, 40, false, true),
            ["api › build #2", "api › build #1", "api › build › seat-a"]
        );
        first.pane.host = "local".into();
        let mut twin = first.clone();
        twin.pane.block_id = "block:02".into();
        let mut elsewhere = first.clone();
        elsewhere.pane.host = "mini".into();
        let rows = vec![first, twin, elsewhere];
        assert_eq!(
            wheres(&rows, 40, true, false),
            ["api › build #1", "api › build #2", "api › build @mini"],
            "no HOST column: @HOST for another host, never @local, and #N where one host has two"
        );
        assert_eq!(
            wheres(&rows, 40, true, true),
            ["api › build #1", "api › build #2", "api › build"],
            "the HOST column already tells the hosts apart"
        );
        for max in [3, 8, 12, 17] {
            let narrow = wheres(&rows, max, true, false);
            assert!(
                narrow[0].ends_with(" #1") && narrow[1].ends_with(" #2"),
                "{narrow:?}"
            );
            assert!(
                narrow[2].ends_with(" @mini"),
                "the suffix is never cut: {narrow:?}"
            );
        }
    }

    #[test]
    fn a_short_tab_stays_whole_and_the_session_gives_way() {
        assert_eq!(path(&["api-gateway", "build"], 14), "api…ay › build");
        let three = ["api-gateway", "build", "seat-a"];
        assert_eq!(path(&three, 25), "api-…way › build › seat-a");
        assert_eq!(path(&three, 23), "api…ay › build › seat-a");
    }
    #[test]
    fn clean_drops_whole_escape_sequences() {
        let text = "a\x1b[31mRED\x1b[0m b\x1b]52;c;aGVsbG8=\x07c\x1b]0;title\x1b\\d\u{9b}2Je\tf\rg";
        assert_eq!(clean(text), "aRED bcde fg");
        assert!(!clean(text).chars().any(char::is_control));
    }

    #[test]
    fn ls_text_never_prints_an_escape_sequence() {
        let row = row("seat-r", "x", "Done \x1b]0;pwned\x07with \x1b[2Jcolour");
        for width in [None, Some(80)] {
            let text = table(std::slice::from_ref(&row), now(), width);
            assert!(!text.contains('\x1b'), "{text:?}");
            assert!(text.contains("Done with colour"), "{text}");
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
