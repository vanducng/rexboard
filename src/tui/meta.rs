//! The card's facts, at its foot: each one once, under a dim label, in aligned columns. Short
//! facts share a row when the card is wide; where and each extension's fields take a row each.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::theme;
use crate::board::Row;
use crate::output::{SEP, clean, fit, harness, labels, path, span};
use crate::pane::bare_title;

/// A fact the card states. Its label is one token, so another set of labels swaps in here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fact {
    Host,
    Harness,
    Turn,
    Title,
    Source,
    Where,
}

impl Fact {
    pub fn label(self) -> &'static str {
        match self {
            Fact::Host => "host",
            Fact::Harness => "harness",
            Fact::Turn => "turn",
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
/// A longer label, such as an extension's name, pushes its own value instead of the column.
const LABEL: usize = 16;

/// The facts for `row` in `width` columns.
pub fn lines(app: &App, row: &Row, width: usize) -> Vec<Line<'static>> {
    let title = bare_title(&row.pane.title);
    // How the agent runs, then what the pane calls itself: each set starts its own row.
    let runs = labelled([
        Some((Fact::Host, row.pane.host.clone())),
        Some((Fact::Harness, harness(row))),
        row.last_turn.map(|secs| (Fact::Turn, span(secs))),
    ]);
    let names = labelled([
        (!title.is_empty()).then(|| (Fact::Title, clean(title))),
        Some((Fact::Source, row.pane.source.clone())),
    ]);
    let groups = extension_groups(app, row);
    let short = || runs.iter().chain(&names);
    let label = short()
        .map(|(l, _)| l.width())
        .chain([Fact::Where.label().width()])
        .chain(groups.iter().map(|(l, _)| l.width()))
        .max()
        .unwrap_or(0)
        .min(LABEL);
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
    let mut lines = grid(&runs);
    lines.push(long_line(
        Fact::Where.label(),
        &place(row, long),
        label,
        room,
    ));
    lines.extend(groups.iter().map(|(l, v)| long_line(l, v, label, room)));
    lines.extend(grid(&names));
    lines
}

fn labelled(
    facts: impl IntoIterator<Item = Option<(Fact, String)>>,
) -> Vec<(&'static str, String)> {
    facts
        .into_iter()
        .flatten()
        .map(|(fact, value)| (fact.label(), value))
        .collect()
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

fn long_line(label: &str, value: &str, column: usize, room: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(pair(label, value, column, room, true));
    Line::from(spans)
}

/// `session › window › block` in `max` columns. The session and window give way, from the
/// middle; the block's own label is never cut while it fits at all.
fn place(row: &Row, max: usize) -> String {
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

/// Each extension's fields on the row, as one group per extension in `enabled` order, labelled
/// by the extension: `session-prefix`, `project api`. A lone field named like its extension is
/// labelled by it alone: `lane`, `desk`.
fn extension_groups(app: &App, row: &Row) -> Vec<(String, String)> {
    let mut groups: Vec<(&str, Vec<(&str, String)>)> = Vec::new();
    for (field, _) in &app.fields {
        let Some(value) = row.ext.get(&field.id) else {
            continue;
        };
        let ext = field
            .id
            .split_once('.')
            .map_or(field.id.as_str(), |(e, _)| e);
        let shown = (field.name.as_str(), clean(value));
        match groups.iter_mut().find(|(e, _)| *e == ext) {
            Some((_, parts)) => parts.push(shown),
            None => groups.push((ext, vec![shown])),
        }
    }
    groups
        .into_iter()
        .map(|(ext, parts)| match &parts[..] {
            [(name, value)] if *name == ext => (ext.to_owned(), value.clone()),
            _ => (
                ext.to_owned(),
                parts
                    .iter()
                    .map(|(n, v)| format!("{n} {v}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
        })
        .collect()
}
