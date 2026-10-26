//! The full view (`space`): one row's pane across the whole terminal, read again every couple of
//! seconds, its lines as the pane laid them out.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Peek};
use super::{card, theme, vt};
use crate::board::Row;
use crate::output::{age, chop, fit};

/// The footer's keys, most needed first; a narrow terminal drops them from the end.
const KEYS: [(&str, &str); 7] = [
    ("space", "back"),
    ("j/k", "scroll"),
    ("a", "answer"),
    ("enter", "jump"),
    ("g/G", "top, bottom"),
    ("pgup/pgdn", "page"),
    ("q", "quit"),
];

/// How often an open view reads its pane again.
pub const EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub struct Full {
    /// The row it shows.
    pub key: String,
    /// How many lines above the bottom the view is scrolled; 0 follows the pane.
    pub back: usize,
    /// The lines as they were when the view left the bottom, so they hold still while paused.
    pub held: Option<Vec<String>>,
    /// When the last read was asked, and whether it is still out.
    pub asked: Instant,
    pub reading: bool,
    /// Why the last read failed; the lines it had stay up.
    pub error: Option<String>,
}

impl Full {
    pub fn new(key: String, now: Instant) -> Self {
        Self {
            key,
            back: 0,
            held: None,
            asked: now,
            reading: true,
            error: None,
        }
    }

    /// Time for the next read: the last one is back and EVERY has passed since it was asked. A
    /// slow host is never asked twice at once, so a newer read never cancels it.
    pub fn due(&self, now: Instant) -> bool {
        !self.reading && now.duration_since(self.asked) >= EVERY
    }

    /// Scrolls `by` lines, older when positive, over `lines` shown `page` at a time. Leaving the
    /// bottom holds the lines still; coming back to it follows the pane again.
    pub fn scroll(&mut self, by: isize, lines: &[String], page: usize) {
        let most = lines.len().saturating_sub(page);
        let back = self.back.saturating_add_signed(by).min(most);
        if back > 0 && self.held.is_none() {
            self.held = Some(lines.to_vec());
        }
        if back == 0 {
            self.held = None;
        }
        self.back = back;
    }

    pub fn following(&self) -> bool {
        self.back == 0
    }
}

/// The lines the view shows: the held ones while paused, else the latest read's.
pub fn lines<'a>(full: &'a Full, peek: Option<&'a Peek>) -> &'a [String] {
    if let Some(held) = &full.held {
        return held;
    }
    match peek
        .filter(|peek| peek.key == full.key)
        .and_then(|p| p.lines.as_ref())
    {
        Some(Ok(lines)) => lines,
        _ => &[],
    }
}

/// The whole terminal: the row's bar, the pane's lines cut at the edge, and the keys.
pub fn draw(frame: &mut Frame, app: &App, full: &Full, row: &Row, now: DateTime<Utc>) {
    let peek = app.peek.as_ref();
    let area = frame.area();
    let width = usize::from(area.width);
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(card::bar(
            row,
            Some(right(full, peek, now, width)),
            now,
            width,
        )),
        top,
    );
    let all = lines(full, peek);
    let room = usize::from(body.height);
    let end = all.len().saturating_sub(full.back);
    let mut shown: Vec<Line<'static>> = all[end.saturating_sub(room)..end]
        .iter()
        .map(|line| cut(line, width))
        .collect();
    if all.is_empty() {
        shown.push(match &full.error {
            Some(why) => Line::styled(fit(&format!(" {why}"), width), Style::new().fg(Color::Red)),
            None => Line::styled(" capturing…", theme::dim()),
        });
    }
    frame.render_widget(Paragraph::new(shown), body);
    let footer = super::view::note(app).map_or_else(
        || keys(width),
        |(text, style)| Line::styled(fit(text, width), style),
    );
    frame.render_widget(Paragraph::new(footer), bottom);
}

/// A line in `width` columns with the pane's colours: cut, not wrapped, so the pane's own layout
/// holds, and a cut one ends in the board's dim `…`. Cutting works on the styled text, so no
/// escape sequence is ever split.
fn cut(line: &str, width: usize) -> Line<'static> {
    let mut spans = vt::spans(line);
    // Padding a program printed after its text, which shows nothing, must not count as cut.
    while let Some(last) = spans.last_mut()
        && last.style.bg.is_none()
        && !last.style.add_modifier.contains(Modifier::REVERSED)
        && last.content.ends_with(' ')
    {
        let trimmed = last.content.trim_end().to_owned();
        if !trimmed.is_empty() {
            last.content = trimmed.into();
            break;
        }
        spans.pop();
    }
    if spans.iter().map(|s| s.content.width()).sum::<usize>() <= width {
        return Line::from(spans);
    }
    // Only coloured padding past the edge, as a diff row's background: nothing to mark.
    let marked = !hidden_is_blank(&spans, width);
    let mut room = if marked {
        width.saturating_sub(1).max(1)
    } else {
        width
    };
    let mut kept = Vec::new();
    for span in spans {
        let w = span.content.width();
        if w <= room {
            room -= w;
            kept.push(span);
            continue;
        }
        let part = chop(&span.content, room.max(1)).swap_remove(0);
        if room > 0 && part.width() <= room {
            kept.push(Span::styled(part, span.style));
        }
        break;
    }
    if marked {
        kept.push(Span::styled("…", theme::dim()));
    }
    Line::from(kept)
}

/// Whether everything past `width` columns is spaces.
fn hidden_is_blank(spans: &[Span], width: usize) -> bool {
    let mut used = 0;
    spans
        .iter()
        .flat_map(|span| span.content.chars())
        .skip_while(|c| {
            used += c.width().unwrap_or(0);
            used <= width
        })
        .all(|c| c == ' ')
}

fn keys(width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, what) in KEYS {
        let sep = if spans.is_empty() { "" } else { " · " };
        let piece = sep.width() + key.width() + 1 + what.width();
        if used + piece > width {
            break;
        }
        used += piece;
        spans.push(Span::styled(sep, theme::dim()));
        spans.push(Span::styled(key, theme::bright()));
        spans.push(Span::styled(format!(" {what}"), theme::dim()));
    }
    Line::from(spans)
}

/// `live · 1s · following`; while paused, whether newer lines came; after a failed read, why,
/// cut so the row's name keeps its room, and how old the lines on screen are.
fn right(full: &Full, peek: Option<&Peek>, now: DateTime<Utc>, width: usize) -> String {
    let Some((peek, at)) = peek
        .filter(|peek| peek.key == full.key)
        .and_then(|peek| Some((peek, peek.at?)))
    else {
        return "capturing…".to_owned();
    };
    let old = age(&at.to_rfc3339(), now);
    if let Some(why) = &full.error {
        let rest = if matches!(peek.lines, Some(Ok(_))) {
            format!(" · {old} old · retrying")
        } else {
            " · retrying".to_owned()
        };
        let room = (width / 2).saturating_sub(rest.width());
        return format!("{}{rest}", fit(why, room).trim_end());
    }
    if full.following() {
        return format!("live · {old} · following");
    }
    let newer = matches!((&peek.lines, &full.held), (Some(Ok(lines)), Some(held)) if lines != held);
    if newer {
        "paused · newer below · G follows".to_owned()
    } else {
        "paused · G follows".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn full_scrolling_up_holds_the_lines_and_g_follows_again() {
        let mut full = Full::new("mini/seat-b".into(), Instant::now());
        let all = lines(50);
        full.scroll(3, &all, 20);
        assert_eq!(full.back, 3);
        assert!(!full.following());
        assert_eq!(full.held.as_deref(), Some(&all[..]), "held as they were");
        full.scroll(1000, &all, 20);
        assert_eq!(full.back, 30, "no further than the top page");
        full.scroll(-1000, &all, 20);
        assert!(
            full.following() && full.held.is_none(),
            "back at the bottom it follows"
        );
    }

    #[test]
    fn full_reads_again_only_once_the_last_read_is_back() {
        let t0 = Instant::now();
        let mut full = Full::new("k".into(), t0);
        assert!(!full.due(t0 + EVERY), "the first read is still out");
        full.reading = false;
        assert!(!full.due(t0 + Duration::from_secs(1)));
        assert!(full.due(t0 + EVERY));
    }
}
