//! The turn card: the selected row in full, under the list at every width. In order: who and
//! where with how the turn ended, the headline, the ask or the body, then the pane's tail.

use chrono::{DateTime, Local, Utc};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::{App, Peek, Spot};
use super::screen::Screen;
use super::theme;
use super::view::key_rows;
use crate::board::Row;
use crate::output::{BREAK, age, chop, clean, fit, in_state, name, path, state_word};
use crate::pane::bare_title;

/// Text wraps at this width however wide the board, so a line stays readable.
const TEXT: usize = 120;
/// One entry of the key table: the key, a space and what it does.
const KEY_COLUMN: usize = 40;

/// The card's lines for a board `width` wide, with the tail filling up to `room` lines.
pub fn lines(app: &App, now: DateTime<Utc>, width: usize, room: usize) -> Vec<Line<'static>> {
    if let Some(answer) = &app.answer {
        return answer.lines(now, width, room);
    }
    if app.keys {
        return keys(app, width);
    }
    let Some(row) = app.current() else {
        return match &app.selected {
            Some(Spot::Group(group)) => vec![Line::styled(
                fit(&format!("{} is folded · za opens it", group.title()), width),
                theme::quiet(),
            )],
            _ => Vec::new(),
        };
    };
    let peek = app.peek.as_ref().filter(|peek| peek.key == row.key);
    let title = bare_title(&row.pane.title);
    let text = width.clamp(1, TEXT);
    let sent = app
        .sent
        .as_ref()
        .filter(|sent| sent.key == row.key)
        .map(|sent| sent.text.as_str());
    let right = sent.map(str::to_owned).or_else(|| fresh(row, peek, now));
    let mut lines = vec![bar(row, right, now, width, false)];
    let msg = clean(row.pane.msg.as_deref().unwrap_or_default());
    let mut segments = msg.split(BREAK).map(str::trim).filter(|s| !s.is_empty());
    match segments.next() {
        None if row.pane.source == "inferred" => {
            lines.push(Line::styled(" no status from this pane", theme::strong()));
            lines.push(Line::styled(
                fit("   space captures the screen", text),
                theme::quiet(),
            ));
        }
        headline => {
            let headline = headline.unwrap_or(if title.is_empty() { "-" } else { title });
            lines.extend(indented(headline, " ", text, theme::strong()));
            for segment in segments {
                lines.extend(indented(segment, "   ", text, theme::plain()));
            }
        }
    }
    if let Some(why) = &app.failed {
        lines.extend(indented(why, " ", text, theme::alert()));
    }
    if let Some(why) = &row.suspect_reason {
        lines.extend(indented(
            &format!("may be stale: {why}"),
            " ",
            text,
            theme::quiet(),
        ));
    }
    let mut meta = super::meta::lines(app, row, text);
    meta.insert(0, Line::default());
    // A blocked row's ask and tail are what it asks, so they are never cut for the facts; the
    // facts take the rows left. Any other message gives way to the facts, down to its headline.
    let blocked = row.pane.state == "blocked";
    if room > 0 && !blocked {
        let over = (lines.len() + meta.len()).saturating_sub(room);
        shorten(&mut lines, over, text);
    }
    let tail = peek.map_or_else(Vec::new, |peek| {
        let mut rows = room.saturating_sub(lines.len());
        if !blocked {
            rows = rows.saturating_sub(meta.len());
        } else if let Some(dialog) = dialog_rows(peek, text) {
            // The dialog is the ask; the output above it gives way to the facts.
            rows = rows.min(dialog);
        }
        tail(peek, text, rows)
    });
    if room > 0 {
        let left = room.saturating_sub(lines.len() + tail.len());
        // The blank line alone is no facts.
        meta.truncate(if left < 2 { 0 } else { left });
    }
    lines.extend(tail);
    lines.extend(meta);
    lines
}

/// The rows a tail's dialog takes `width` wide, from its opening to the end, as `tail` draws
/// them; None when the tail shows no dialog.
fn dialog_rows(peek: &Peek, width: usize) -> Option<usize> {
    let Some(Ok(lines)) = &peek.lines else {
        return None;
    };
    // The parse drops blank lines at the end, which the tail still draws.
    let blank = lines
        .iter()
        .rev()
        .take_while(|l| l.trim().is_empty())
        .count();
    let count = Screen::parse(&lines.join("\n"), None).dialog? + blank;
    let room = width.saturating_sub(theme::GUTTER.width()).max(1);
    Some(
        lines[lines.len().saturating_sub(count)..]
            .iter()
            .map(|line| chop(&clean(line), room).len())
            .sum(),
    )
}

/// The message `over` lines shorter, keeping the bar and the headline; the last kept line ends
/// in `…`.
fn shorten(lines: &mut Vec<Line<'static>>, over: usize, width: usize) {
    let keep = lines.len().saturating_sub(over).max(2);
    if keep >= lines.len() {
        return;
    }
    lines.truncate(keep);
    if let Some(last) = lines.last_mut() {
        let text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        let style = last.spans.first().map_or(last.style, |s| s.style);
        *last = Line::styled(
            fit(&format!("{}…", text.trim_end()), width)
                .trim_end()
                .to_owned(),
            style,
        );
    }
}

/// The first line: the row's state, its name and how long it has been so, then how fresh the
/// tail is, or the answer the board last sent it. The full view, which has no list or facts
/// beside it, also names where the row is and its host.
/// `━━ ✓ done · seat-a · ended 4m ago ━━━━━━ captured …`
pub(super) fn bar(
    row: &Row,
    right: Option<String>,
    now: DateTime<Utc>,
    width: usize,
    placed: bool,
) -> Line<'static> {
    let mut state = state_word(row);
    if let Some(kind) = &row.pane.kind {
        state = format!("{state} · {kind}");
    }
    let ends = theme::BAR_END;
    let right = right.map_or_else(|| ends.to_owned(), |text| format!(" {text} {ends}"));
    let room = width.saturating_sub(ends.width() + right.width() + 1);
    let line = |place: Option<&str>, extra: &[String]| {
        let mut parts = vec![name(row)];
        parts.extend(place.map(str::to_owned));
        parts.extend_from_slice(extra);
        parts.extend(outcome(row, now));
        clean(&format!(
            " {} {state} · {} ",
            theme::glyph(row, now),
            parts.join(" · ")
        ))
    };
    let left = if placed {
        // On a narrow bar the host gives way first, then the place, before the outcome.
        let labels = [row.pane.session.as_str(), row.pane.tab.as_str()];
        let least = path(&labels, usize::MAX).width().min(12);
        let mut extra = vec![row.pane.host.clone()];
        while !extra.is_empty() && line(Some(""), &extra).width() + least > room {
            extra.pop();
        }
        match room.checked_sub(line(Some(""), &extra).width()) {
            Some(place) if place >= least => line(Some(&path(&labels, place)), &extra),
            _ => line(None, &extra),
        }
    } else {
        line(None, &[])
    };
    let left = fit(&left, room).trim_end().to_owned() + " ";
    let fill = width.saturating_sub(ends.width() + left.width() + right.width());
    let mut spans = vec![Span::styled(ends, theme::quiet())];
    // The glyph keeps its own face: in a font with no bold braille a bold spinner is a still blob.
    let (glyph, bold) = (theme::glyph(row, now), theme::bar_label(row));
    match left.strip_prefix(&format!(" {glyph}")) {
        Some(label) => spans.extend([
            Span::styled(" ", bold),
            Span::styled(glyph, theme::state(row)),
            Span::styled(label.to_owned(), bold),
        ]),
        None => spans.push(Span::styled(left, bold)),
    }
    spans.extend([
        Span::styled(theme::RULE.repeat(fill), theme::quiet()),
        Span::styled(right, theme::quiet()),
    ]);
    Line::from(spans)
}

/// How the turn stands: `ended 4m ago`, `waiting 12s`, `for 2m`. Nothing for a
/// row with no report to date it.
fn outcome(row: &Row, now: DateTime<Utc>) -> Option<String> {
    let since = in_state(row, now);
    if since == "-" {
        return None;
    }
    Some(match row.pane.state.as_str() {
        "done" => format!("ended {since} ago"),
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
/// off the top. A gutter marks them as the pane's own screen, not the agent's report.
pub fn tail(peek: &Peek, width: usize, room: usize) -> Vec<Line<'static>> {
    match &peek.lines {
        None => vec![Line::styled(" capturing…", theme::quiet())],
        Some(Ok(text)) => {
            let mut rows = Vec::new();
            for line in text.iter().rev() {
                if rows.len() >= room {
                    break;
                }
                rows.extend(
                    // The full view's reads keep the pane's colours, which the card drops.
                    chop(
                        &clean(line),
                        width.saturating_sub(theme::GUTTER.width()).max(1),
                    )
                    .into_iter()
                    .rev(),
                );
            }
            rows.truncate(room);
            rows.into_iter()
                .rev()
                .map(|line| Line::styled(format!("{}{line}", theme::GUTTER), theme::quiet()))
                .collect()
        }
        // An error is worth reading whole, so it wraps instead of being cut.
        Some(Err(why)) => indented(why, " ", width, theme::alert()),
    }
}

/// The `?` table, in as many columns as fit, filled down each column.
fn keys(app: &App, width: usize) -> Vec<Line<'static>> {
    let all = key_rows(app);
    let columns = (width / KEY_COLUMN).max(1);
    let rows = all.len().div_ceil(columns);
    let motion = " · \"motion\": \"reduced\" in tui.json stills the spinner";
    let title = if " keys".width() + motion.width() <= width {
        Line::from(vec![
            Span::styled(" keys", theme::accent()),
            Span::styled(motion, theme::quiet()),
        ])
    } else {
        Line::styled(fit(" keys", width), theme::accent())
    };
    let mut lines = vec![title];
    for r in 0..rows {
        let mut spans = Vec::new();
        for c in 0..columns {
            let Some((key, what)) = all.get(c * rows + r) else {
                break;
            };
            spans.push(Span::styled(format!(" {key:<8} "), theme::strong()));
            let room = KEY_COLUMN.min(width).saturating_sub(10);
            spans.push(Span::styled(fit(what, room), theme::quiet()));
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
