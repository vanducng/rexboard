//! The card's facts, at its foot: each one once, under a dim label, in aligned columns. Short
//! facts share a row when the card is wide; where and each extension's fields take a row each.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::columns::{Col, Columns, Tier, opted};
use super::icons;
use super::theme;
use super::views::Key;
use crate::board::Row;
use crate::client::Field;
use crate::output::{
    SEP, blocks, clean, context, dollars, fit, harness, labels, path, span, squeeze, wheres,
};
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
                Fact::Harness => row
                    .pane
                    .harness
                    .as_deref()
                    .and_then(crate::harness::mark)
                    .map_or("harness", |i| i.glyph),
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

/// A track's value, in columns; the label cell before it is 7 in words and 1 in glyphs.
const VALUE: usize = 20;
/// The columns between tracks.
const GAP: usize = 3;
/// The card width from which two and three tracks fit a row, in words and with glyphs.
const WORD_TRACKS: [usize; 2] = [59, 91];
const GLYPH_TRACKS: [usize; 2] = [47, 73];

/// The facts for `row` in `width` columns of a `board` as wide, as a grid: each fact a track of
/// a label and a value, one to three tracks to a row; the facts the list row already shows in a
/// visible column are left out.
pub fn lines(app: &App, row: &Row, width: usize, board: usize) -> Vec<Line<'static>> {
    let columns = app.columns(u16::try_from(board).unwrap_or(u16::MAX), board);
    // A column is on screen when the list draws it whole, not when it only sits in `columns`.
    let drawn = |id| columns.drawn(id, board);
    let title = bare_title(&row.pane.title);
    let usage = row.usage.as_ref();
    let fact = |fact: Fact, value: String| (fact.label(row, app.nerd).to_owned(), value);
    let opt = |col| opted(&app.columns, col);
    let runs: Vec<_> = [
        (!drawn(Col::Host)).then(|| fact(Fact::Host, clean(&row.pane.host))),
        (!drawn(Col::Harness)).then(|| fact(Fact::Harness, harness(row))),
        row.last_turn
            .filter(|_| !drawn(Col::Turn))
            .map(|s| fact(Fact::Turn, span(s))),
        usage
            .and_then(|u| u.model.clone())
            .map(|m| fact(Fact::Model, clean(&m))),
        usage.and_then(context).map(|c| fact(Fact::Ctx, c)),
        usage
            .and_then(|u| u.cost_usd)
            .filter(|_| opt(Col::Cost))
            .map(|c| fact(Fact::Cost, dollars(c))),
        usage
            .and_then(|u| u.quota_5h)
            .filter(|_| opt(Col::Quota))
            .map(|p| fact(Fact::Quota, format!("{p}%"))),
    ]
    .into_iter()
    .flatten()
    .collect();
    // An extension's field is a fact named after it, unless its column is on screen.
    let fields: Vec<(String, String)> = app
        .fields
        .iter()
        .filter(|(f, _)| {
            !columns.cols.iter().any(|(c, _)| {
                matches!(c.id, Col::Field(id) if id == f.id) && columns.drawn(c.id, board)
            })
        })
        .filter_map(|(f, _)| Some((field_label(f, app.nerd), clean(row.ext.get(&f.id)?))))
        .collect();
    let repeats = title.is_empty() || title == clean(&crate::output::name(row));
    let names = vec![
        (!repeats).then(|| fact(Fact::Title, clean(title))),
        Some(fact(Fact::Source, row.pane.source.clone())),
    ]
    .into_iter()
    .flatten()
    .collect();
    // A value that cleans to nothing is no fact: its label would stand over a blank.
    let groups = [runs, fields, names].map(|group: Vec<(String, String)>| {
        group
            .into_iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .collect::<Vec<_>>()
    });
    // Glyph labels are one cell; a word among them, as an extension field with no mark, keeps the
    // words' label cell for the whole card.
    let glyphs = app.nerd && groups.iter().flatten().all(|(l, _)| l.width() == 1);
    let (label, thresholds) = if glyphs {
        (1, GLYPH_TRACKS)
    } else {
        (7, WORD_TRACKS)
    };
    let across = 1 + thresholds.iter().filter(|t| width >= **t).count();
    let room = width.saturating_sub(1);
    let mut lines = Vec::new();
    if !list_shows_place(app, &columns, row, board) {
        let long = room.saturating_sub(label + 1);
        let place_label = Fact::Where.label(row, app.nerd);
        let mut spans = vec![Span::raw(" ")];
        spans.extend(cell(
            place_label,
            &place(row, long, app.nerd),
            label,
            long,
            true,
        ));
        lines.push(Line::from(spans));
    }
    for group in groups {
        for chunk in group.chunks(across) {
            let mut spans = vec![Span::raw(" ")];
            for (i, (l, v)) in chunk.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" ".repeat(GAP)));
                }
                // A value is cut at its track, and at the row's edge when the track would pass it;
                // alone on its row it may run to the edge.
                let left = room.saturating_sub(i * (label + 1 + VALUE + GAP) + label + 1);
                let shown = if across == 1 { left } else { VALUE.min(left) };
                spans.extend(cell(l, v, label, shown, i + 1 == chunk.len()));
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}

/// An extension field's label: its own name, or with glyphs its declared mark.
fn field_label(field: &Field, nerd: bool) -> String {
    match (nerd, field.icon.as_deref()) {
        (true, Some(icon)) => icon.to_owned(),
        _ => field.name.clone(),
    }
}

/// Whether the list row already says where the pane is: its SESSION, WINDOW and BLOCK, or WHERE
/// with all three labels, drawn whole as the list draws them, twin suffixes and cuts included.
fn list_shows_place(app: &App, columns: &Columns, row: &Row, board: usize) -> bool {
    let Some(at) = app.rows.iter().position(|r| r.key == row.key) else {
        return false;
    };
    let drawn = |id| columns.drawn(id, board);
    let room = |id| columns.width(id).saturating_sub(1);
    let whole = labels(row, false);
    if drawn(Col::Session) && drawn(Col::Window) && drawn(Col::Block) {
        let block = blocks(
            &app.rows,
            room(Col::Session),
            room(Col::Window),
            room(Col::Block),
        );
        return squeeze(&clean(&row.pane.session), room(Col::Session)) == clean(&row.pane.session)
            && squeeze(&clean(&row.pane.tab), room(Col::Window)) == clean(&row.pane.tab)
            && block[at] == clean(&crate::output::name(row));
    }
    let host = columns.has(Col::Host) || app.view.group_by == Some(Key::Host);
    columns.tier == Tier::Wide
        && drawn(Col::Where)
        && wheres(&app.rows, room(Col::Where), false, host)[at]
            == whole.iter().map(|l| clean(l)).collect::<Vec<_>>().join(SEP)
}

/// A track's label, padded to the label cell, and its value cut to `shown` columns; the last one
/// on a row leaves no trailing spaces.
fn cell(label: &str, value: &str, column: usize, shown: usize, last: bool) -> Vec<Span<'static>> {
    let head = format!("{:<column$} ", squeeze(label, column));
    let mut value = fit(value, shown);
    if last {
        value = value.trim_end().to_owned();
    }
    vec![
        Span::styled(head, theme::quiet()),
        Span::styled(value, theme::plain()),
    ]
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
