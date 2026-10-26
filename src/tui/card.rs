//! The turn card: the selected row in full, under the list at every width. In order: who and
//! where with how the turn ended, the headline, the ask or the body, then the pane's tail.

use chrono::{DateTime, Local, Utc};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::{App, Peek, Spot};
use super::theme;
use super::view::KEYS;
use crate::board::Row;
use crate::output::{
    BREAK, age, chop, clean, fit, harness, in_state, name, path, span, state_word,
};
use crate::pane::bare_title;

/// Text wraps at this width however wide the board, so a line stays readable.
const TEXT: usize = 120;
/// One entry of the key table: the key, a space and what it does.
const KEY_COLUMN: usize = 40;

/// The card's lines for a board `width` wide, with the tail filling up to `room` lines.
pub fn lines(app: &App, now: DateTime<Utc>, width: usize, room: usize) -> Vec<Line<'static>> {
    if app.keys {
        return keys(width);
    }
    let Some(row) = app.current() else {
        return match &app.selected {
            Some(Spot::Group(group)) => vec![Line::styled(
                fit(&format!("{} is folded · z opens it", group.title()), width),
                theme::dim(),
            )],
            _ => Vec::new(),
        };
    };
    let peek = app.peek.as_ref().filter(|peek| peek.key == row.key);
    let title = bare_title(&row.pane.title);
    let text = width.clamp(1, TEXT);
    let mut lines = vec![bar(row, peek, now, width)];
    let msg = clean(row.pane.msg.as_deref().unwrap_or_default());
    let mut segments = msg.split(BREAK).map(str::trim).filter(|s| !s.is_empty());
    match segments.next() {
        None if row.pane.source == "inferred" => {
            lines.push(Line::styled(" no status from this pane", theme::bright()));
            lines.push(Line::styled(
                fit("   space captures the screen", text),
                theme::dim(),
            ));
        }
        headline => {
            let headline = headline.unwrap_or(if title.is_empty() { "-" } else { title });
            lines.extend(indented(headline, " ", text, theme::bright()));
            for segment in segments {
                lines.extend(indented(segment, "   ", text, Style::new()));
            }
        }
    }
    if let Some(why) = &app.failed {
        lines.extend(indented(why, " ", text, Style::new().fg(Color::Red)));
    }
    if let Some(why) = &row.suspect_reason {
        lines.extend(indented(
            &format!("may be stale: {why}"),
            " ",
            text,
            theme::dim(),
        ));
    }
    let mut meta = vec![format!("source {}", row.pane.source)];
    if !title.is_empty() {
        meta.insert(0, format!("title {title}"));
    }
    lines.push(Line::styled(
        fit(&format!(" {}", meta.join(" · ")), text),
        theme::dim(),
    ));
    if let Some(peek) = peek {
        let room = room.saturating_sub(lines.len());
        lines.extend(tail(peek, text, room));
    }
    lines
}

/// The first line: the row's state and place, how the turn went, and how fresh the tail is.
/// `━━ ● done · seat-a · api › main · mini · claude · ended 4m ago · turn 6m12s ━━ captured …`
fn bar(row: &Row, peek: Option<&Peek>, now: DateTime<Utc>, width: usize) -> Line<'static> {
    let mut state = state_word(row);
    if let Some(kind) = &row.pane.kind {
        state = format!("{state} · {kind}");
    }
    let right = fresh(row, peek, now).map_or_else(|| "━━".to_owned(), |text| format!(" {text} ━━"));
    let room = width.saturating_sub("━━".width() + right.width() + 1);
    let line = |place: Option<&str>, extra: &[String]| {
        let mut parts = vec![name(row)];
        parts.extend(place.map(str::to_owned));
        parts.extend_from_slice(extra);
        parts.extend(outcome(row, now));
        clean(&format!(
            " {} {state} · {} ",
            theme::glyph(row),
            parts.join(" · ")
        ))
    };
    // The list row shows the place, host and harness, so on a narrow bar they give way before
    // the outcome.
    let labels = [row.pane.session.as_str(), row.pane.tab.as_str()];
    let least = path(&labels, usize::MAX).width().min(12);
    let mut extra = vec![row.pane.host.clone(), harness(row)];
    while !extra.is_empty() && line(Some(""), &extra).width() + least > room {
        extra.pop();
    }
    let left = match room.checked_sub(line(Some(""), &extra).width()) {
        Some(place) if place >= least => line(Some(&path(&labels, place)), &extra),
        _ => line(None, &extra),
    };
    let left = fit(&left, room).trim_end().to_owned() + " ";
    let fill = width.saturating_sub("━━".width() + left.width() + right.width());
    Line::from(vec![
        Span::styled("━━", theme::dim()),
        Span::styled(left, theme::state(row).add_modifier(Modifier::BOLD)),
        Span::styled("━".repeat(fill), theme::dim()),
        Span::styled(right, theme::dim()),
    ])
}

/// How the turn stands: `ended 4m ago · turn 6m12s`, `waiting 12s`, `for 2m`. Nothing for a
/// row with no report to date it.
fn outcome(row: &Row, now: DateTime<Utc>) -> Option<String> {
    let since = in_state(row, now);
    if since == "-" {
        return None;
    }
    let turn = row.last_turn.map(|secs| format!(" · turn {}", span(secs)));
    Some(match row.pane.state.as_str() {
        "done" => format!("ended {since} ago{}", turn.unwrap_or_default()),
        "blocked" => format!("waiting {since}"),
        "error" => format!("failed {since} ago"),
        _ => format!("for {since}"),
    })
}

/// `captured 11:21:14 · 8s` for a tail, or `newer report · space refreshes` once the row has
/// reported since the tail was taken.
fn fresh(row: &Row, peek: Option<&Peek>, now: DateTime<Utc>) -> Option<String> {
    let peek = peek?;
    let Some(at) = peek.at else {
        return Some("capturing…".to_owned());
    };
    let parsed = |at: Option<&str>| at.and_then(|at| DateTime::parse_from_rfc3339(at).ok());
    if let (Some(report), Some(asked)) = (
        parsed(row.pane.updated_at.as_deref()),
        parsed(peek.updated_at.as_deref()),
    ) && report > asked
    {
        return Some("newer report · space refreshes".to_owned());
    }
    Some(format!(
        "captured {} · {}",
        at.with_timezone(&Local).format("%H:%M:%S"),
        age(&at.to_rfc3339(), now)
    ))
}

/// `text` wrapped to `width` after `lead`, the lines past the first under it; a word longer than
/// the line is cut into pieces, so nothing runs off the card.
fn indented(text: &str, lead: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let room = width.saturating_sub(lead.width()).max(1);
    wrap(text, room)
        .into_iter()
        .flat_map(|line| chop(&line, room))
        .map(|line| Line::styled(format!("{lead}{line}"), style))
        .collect()
}

/// The newest `room` rows of the tail, wrapped, so the last lines show whole and the oldest fall
/// off the top.
pub fn tail(peek: &Peek, width: usize, room: usize) -> Vec<Line<'static>> {
    match &peek.lines {
        None => vec![Line::styled(" capturing…", theme::dim())],
        Some(Ok(text)) => {
            let mut rows = Vec::new();
            for line in text.iter().rev() {
                if rows.len() >= room {
                    break;
                }
                rows.extend(chop(line, width.saturating_sub(1).max(1)).into_iter().rev());
            }
            rows.truncate(room);
            rows.into_iter()
                .rev()
                .map(|line| Line::styled(format!(" {line}"), theme::dim()))
                .collect()
        }
        // An error is worth reading whole, so it wraps instead of being cut.
        Some(Err(why)) => indented(why, " ", width, Style::new().fg(Color::Red)),
    }
}

/// The `?` table, in as many columns as fit, filled down each column.
fn keys(width: usize) -> Vec<Line<'static>> {
    let columns = (width / KEY_COLUMN).max(1);
    let rows = KEYS.len().div_ceil(columns);
    let mut lines = vec![Line::styled(fit(" keys", width), theme::accent())];
    for r in 0..rows {
        let mut spans = Vec::new();
        for c in 0..columns {
            let Some((key, what)) = KEYS.get(c * rows + r) else {
                break;
            };
            spans.push(Span::styled(format!(" {key:<7} "), theme::bright()));
            let room = KEY_COLUMN.min(width).saturating_sub(9);
            spans.push(Span::styled(fit(what, room), theme::dim()));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Words joined into lines of at most `width` columns.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let last = lines.last_mut().expect("one line");
        if !last.is_empty() && last.width() + 1 + word.width() > width {
            lines.push(String::new());
        }
        let last = lines.last_mut().expect("one line");
        if !last.is_empty() {
            last.push(' ');
        }
        last.push_str(word);
    }
    lines
}
