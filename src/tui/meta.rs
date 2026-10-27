//! The card's facts, at its foot: each one once, under a dim label, in aligned columns. Short
//! facts share a row when the card is wide; where and each extension's fields take a row each.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::columns::{Col, opted};
use super::icons;
use super::theme;
use crate::board::Row;
use crate::client::Field;
use crate::output::{SEP, clean, context, dollars, fit, harness, labels, path, span};
use crate::pane::bare_title;

/// A fact the card states. Its label is one token: a word, or with `"icons": "nerd"` a glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fact {
    Host,
    Harness,
    Turn,
    Model,
    Ctx,
    Cost,
    Quota,
    Title,
    Source,
    Where,
}

impl Fact {
    pub fn label(self, row: &Row, nerd: bool) -> &'static str {
        if nerd {
            return match self {
                Fact::Host => icons::HOST.glyph,
                Fact::Harness => icons::harness(row.pane.harness.as_deref()).glyph,
                Fact::Turn => icons::TURN.glyph,
                Fact::Model => icons::MODEL.glyph,
                Fact::Ctx => icons::CTX.glyph,
                Fact::Cost => icons::COST.glyph,
                Fact::Quota => icons::QUOTA.glyph,
                Fact::Title => icons::TITLE.glyph,
                Fact::Source => icons::source(&row.pane.source).map_or("source", |i| i.glyph),
                // The session's mark leads `where`, whose window and block carry their own.
                Fact::Where => icons::SESSION.glyph,
            };
        }
        match self {
            Fact::Host => "host",
            Fact::Harness => "harness",
            Fact::Turn => "turn",
            Fact::Model => "model",
            Fact::Ctx => "ctx",
            Fact::Cost => "cost",
            Fact::Quota => "5h",
            Fact::Title => "title",
            Fact::Source => "source",
            Fact::Where => "where",
        }
    }
}

/// The most a short fact shows before it is cut, so a long title cannot push the columns apart.
const VALUE: usize = 24;
/// Short facts to a row at most, and the columns between them.
const ACROSS: usize = 3;
const GAP: usize = 3;

/// The facts for `row` in `width` columns.
pub fn lines(app: &App, row: &Row, width: usize) -> Vec<Line<'static>> {
    let title = bare_title(&row.pane.title);
    let usage = row.usage.as_ref();
    // How the agent runs, then what the pane calls itself: each set starts its own row.
    let labelled = |facts: Vec<Option<(Fact, String)>>| -> Vec<(&'static str, String)> {
        facts
            .into_iter()
            .flatten()
            .map(|(fact, value)| (fact.label(row, app.nerd), value))
            .collect()
    };
    let runs = labelled(vec![
        Some((Fact::Host, row.pane.host.clone())),
        Some((Fact::Harness, harness(row))),
        row.last_turn.map(|secs| (Fact::Turn, span(secs))),
        usage
            .and_then(|u| u.model.clone())
            .map(|m| (Fact::Model, clean(&m))),
        usage.and_then(context).map(|c| (Fact::Ctx, c)),
        usage
            .and_then(|u| u.cost_usd)
            .filter(|_| opted(&app.columns, Col::Cost))
            .map(|c| (Fact::Cost, dollars(c))),
        usage
            .and_then(|u| u.quota_5h)
            .filter(|_| opted(&app.columns, Col::Quota))
            .map(|p| (Fact::Quota, format!("{p}%"))),
    ]);
    let names = labelled(vec![
        (!title.is_empty()).then(|| (Fact::Title, clean(title))),
        Some((Fact::Source, row.pane.source.clone())),
    ]);
    let groups = extension_groups(app, row);
    let place_label = Fact::Where.label(row, app.nerd);
    let short = || runs.iter().chain(&names);
    // An extension's name runs on past the column, so a long one widens only its own row.
    let label = short()
        .map(|(l, _)| l.width())
        .chain([place_label.width()])
        .max()
        .unwrap_or(0);
    let room = width.saturating_sub(1);
    let value = short()
        .map(|(_, v)| v.width())
        .max()
        .unwrap_or(0)
        .min(VALUE);
    let cell = label + 1 + value;
    let across = ((room + GAP) / (cell + GAP)).clamp(1, ACROSS);
    // Alone on its row, a fact may run to the edge.
    let shown = if across == 1 { room } else { cell };
    let grid = |facts: &[(&str, String)]| -> Vec<Line<'static>> {
        facts
            .chunks(across)
            .map(|row| {
                let mut spans = vec![Span::raw(" ")];
                for (i, (l, v)) in row.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw(" ".repeat(GAP)));
                    }
                    spans.extend(pair(l, v, label, shown, i + 1 == row.len()));
                }
                Line::from(spans)
            })
            .collect()
    };
    let long = room.saturating_sub(label + 1);
    // Where first: it is what tells two rows of the same name apart.
    let place = place(row, long, app.nerd);
    let mut lines = vec![long_line(
        place_label,
        vec![Span::styled(place, theme::plain())],
        label,
        room,
    )];
    lines.extend(grid(&runs));
    lines.extend(
        groups
            .into_iter()
            .map(|(name, parts)| long_line(&name, parts, label, room)),
    );
    lines.extend(grid(&names));
    lines
}

/// `label value` in `cell` columns, the label padded to the label column; the last on a row
/// leaves no trailing spaces.
fn pair(label: &str, value: &str, column: usize, cell: usize, last: bool) -> Vec<Span<'static>> {
    let head = format!("{label:<column$} ");
    let room = cell.saturating_sub(head.width()).max(1);
    let mut shown = fit(value, room);
    if last {
        shown = shown.trim_end().to_owned();
    }
    vec![
        Span::styled(head, theme::quiet()),
        Span::styled(shown, theme::plain()),
    ]
}

/// A row of its own: a label, dim and padded to the label column or running past it, then
/// `parts`, cut to `room`.
fn long_line(label: &str, parts: Vec<Span<'static>>, column: usize, room: usize) -> Line<'static> {
    let head = Span::styled(format!("{label:<column$} "), theme::quiet());
    let mut left = room;
    let mut spans = vec![Span::raw(" ")];
    for span in std::iter::once(head).chain(parts) {
        let wide = span.content.width();
        if wide <= left {
            left -= wide;
            spans.push(span);
            continue;
        }
        if left > 0 {
            spans.push(Span::styled(
                fit(&span.content, left).trim_end().to_owned(),
                span.style,
            ));
        }
        break;
    }
    Line::from(spans)
}

/// `session › window › block` in `max` columns; with glyphs the window and the block each lead
/// with their own, unless the place is cut down past its labels.
fn place(row: &Row, max: usize, nerd: bool) -> String {
    if !nerd {
        return plain_place(row, max);
    }
    let marks = [icons::WINDOW.glyph, icons::BLOCK.glyph];
    let count = labels(row, false).len();
    let plain = plain_place(row, max.saturating_sub(2 * (count - 1)));
    let parts: Vec<&str> = plain.split(SEP).collect();
    if parts.len() != count {
        return plain;
    }
    let marked: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| match i {
            0 => (*part).to_owned(),
            _ => format!("{} {part}", marks[i - 1]),
        })
        .collect();
    marked.join(SEP)
}

/// `session › window › block` in `max` columns. The session and window give way, from the
/// middle; the block's own label is never cut while it fits at all.
fn plain_place(row: &Row, max: usize) -> String {
    let all = labels(row, false);
    let [session, window, block] = all[..] else {
        return path(&all, max);
    };
    let block = clean(block);
    match max.checked_sub(block.width() + SEP.width()) {
        Some(room) if room >= 3 => format!("{}{SEP}{block}", path(&[session, window], room)),
        _ => fit(&block, max).trim_end().to_owned(),
    }
}

/// Each extension's fields on the row, as one group per extension in `enabled` order: its name
/// (`session-prefix`), then each field's name and value (`project api`). A lone field named like
/// its extension shows only its value (`lane`, `desk`). With glyphs a declared mark stands for the
/// extension and for a field's name.
fn extension_groups(app: &App, row: &Row) -> Vec<(String, Vec<Span<'static>>)> {
    let mut groups: Vec<(&str, Vec<(&Field, String)>)> = Vec::new();
    for (field, _) in &app.fields {
        let Some(value) = row.ext.get(&field.id) else {
            continue;
        };
        let ext = field
            .id
            .split_once('.')
            .map_or(field.id.as_str(), |(e, _)| e);
        let shown = (field, clean(value));
        match groups.iter_mut().find(|(e, _)| *e == ext) {
            Some((_, parts)) => parts.push(shown),
            None => groups.push((ext, vec![shown])),
        }
    }
    let quiet = |text: String| Span::styled(text, theme::quiet());
    let mark = |field: &Field| match (app.nerd, field.icon.as_deref()) {
        (true, Some(icon)) => icon.to_owned(),
        _ => field.name.clone(),
    };
    groups
        .into_iter()
        .map(|(ext, parts)| {
            let mut spans = Vec::new();
            match &parts[..] {
                [(field, value)] if field.name == ext => {
                    spans.push(Span::styled(value.clone(), theme::plain()));
                }
                _ => {
                    for (i, (field, value)) in parts.iter().enumerate() {
                        if i > 0 {
                            spans.push(quiet(" · ".to_owned()));
                        }
                        spans.push(quiet(format!("{} ", mark(field))));
                        spans.push(Span::styled(value.clone(), theme::plain()));
                    }
                }
            }
            if !app.nerd {
                return (ext.to_owned(), spans);
            }
            match parts[0].0.extension_icon.as_deref() {
                Some(icon) => (icon.to_owned(), spans),
                // The package mark cannot say which extension it is, so the name leads the value.
                None => {
                    spans.insert(0, quiet(format!("{ext} · ")));
                    (icons::EXTENSION.glyph.to_owned(), spans)
                }
            }
        })
        .collect()
}
