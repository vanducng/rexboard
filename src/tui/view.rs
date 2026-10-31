use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use ratatui::crossterm::event::KeyCode;

use super::app::{App, CardDrawn, Item, Link, NUMBERED, Spot};
use super::columns::{Col, Columns, Sort, Tier, Tone, column_of};
use super::keys::{self, Act};
use super::prefs::CardHeight;
use super::status::ALL;
use super::theme;
use super::views::{Bucket, Key, View};
use crate::board::Row;
use crate::output::{blocks, fit, wheres};

/// Below this many rows the column header and the blank row above it are left out, so no row
/// is lost to them.
pub(super) const HEADED: u16 = 12;
/// The column header's row when there is one: under the top bar and its blank row.
pub(super) const HEADER_ROW: u16 = 2;
/// The list rows the card always leaves.
const LIST_FLOOR: u16 = 3;

const STOPPED: &str = "serve stopped · R reconnects";

/// The `?` table: the board's keymap, and while extensions declare fields, a row for their sort
/// letters after the built-in ones, naming each field and, when it differs, its extension.
pub fn key_rows(app: &App) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = keys::BOARD
        .iter()
        .filter(|entry| !entry.keys.is_empty())
        .map(|entry| (entry.keys.to_owned(), entry.help.to_owned()))
        .collect();
    if app.fields.is_empty() {
        return rows;
    }
    let letters: Vec<String> = app
        .fields
        .iter()
        .filter_map(|(_, l)| *l)
        .map(String::from)
        .collect();
    let named = |sources: bool| {
        let names: Vec<String> = app
            .fields
            .iter()
            .map(|(f, _)| match f.id.split_once('.') {
                Some((ext, _)) if sources && ext != f.name => format!("{} ({ext})", f.name),
                _ => f.name.clone(),
            })
            .collect();
        format!("sort by {}", names.join(", "))
    };
    let what = Some(named(true))
        .filter(|w| w.width() <= KEY_WIDTH)
        .unwrap_or_else(|| named(false));
    let after = rows
        .iter()
        .position(|(k, _)| k == "o")
        .map_or(rows.len(), |i| i + 1);
    rows.insert(after, (format!("o {}", letters.join(" ")), what));
    rows
}

/// The most a `?` description shows uncut.
pub const KEY_WIDTH: usize = 30;

pub fn draw(frame: &mut Frame, app: &App, now: DateTime<Utc>) {
    if let Some(full) = &app.full
        && let Some(row) = app.rows.iter().find(|row| row.key == full.key)
    {
        return super::full::draw(frame, app, full, row, now);
    }
    let area = frame.area();
    app.card_drawn.set(CardDrawn::default());
    app.strip_drawn.set(false);
    let card = app.card_shown();
    let strip = app.strip_shown();
    let [top, _gap, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(area.height >= HEADED)),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);
    header(frame, top, app, now);
    let headed = area.height >= HEADED;
    let height = if card {
        card_height(app, now, body, headed)
    } else if strip {
        // The bar, its lines and the blank row above the footer, leaving the list its floor; too
        // little room shows no strip, and then nothing is read for it.
        let room = body.height.saturating_sub(LIST_FLOOR + u16::from(headed));
        let wanted = 1 + app.tuning.lines() as u16 + FOOTER_GAP;
        let rows = wanted.min(room);
        app.strip_drawn.set(rows >= 3);
        if rows >= 3 { rows } else { 0 }
    } else {
        0
    };
    let [list_area, card_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(body);
    if list_area.height > 0 {
        list(frame, list_area, app, area, now);
    }
    if height > 0 {
        let room = card_area.height.saturating_sub(FOOTER_GAP);
        let width = usize::from(body.width);
        let lines = if card {
            super::card::lines(app, now, width, usize::from(room))
        } else {
            super::card::strip(app, now, width, usize::from(room))
        };
        let card_area = Rect {
            height: room,
            ..card_area
        };
        frame.render_widget(Paragraph::new(lines), card_area);
        let mut drawn = app.card_drawn.get();
        if drawn.last > 0 {
            (drawn.top, drawn.bottom) = (card_area.y, card_area.bottom());
            app.card_drawn.set(drawn);
        }
    }
    footer(frame, bottom, app);
}

/// The blank rows between the card and the footer, taken from the card's own height.
pub const FOOTER_GAP: u16 = 1;

/// The card's rows: the same for every row, so the list holds still under the selection, its
/// text scrolling inside it; the key table's own rows; or for answer mode, enough to show every
/// option. Always leaving the list 3 rows under its column header, when it has one.
fn card_height(app: &App, now: DateTime<Utc>, body: Rect, headed: bool) -> u16 {
    let most = body.height.saturating_sub(LIST_FLOOR + u16::from(headed));
    if let Some(answer) = &app.answer {
        let lines = |rows: u16| {
            let room = usize::from(rows.saturating_sub(FOOTER_GAP));
            super::card::lines(app, now, usize::from(body.width), room).len() as u16 + FOOTER_GAP
        };
        let mut height = fixed_height(app, body, headed).max(lines(most)).min(most);
        // A digit picks only an option the card drew, so the card takes rows from the list,
        // all of them if it must, until every option shows.
        while height < body.height {
            lines(height);
            if answer.all_shown() {
                break;
            }
            height += 1;
        }
        return height;
    }
    // The key table takes only its own rows, so it sits on the footer and the list keeps the rest.
    if app.keys {
        let table = super::card::lines(app, now, usize::from(body.width), 0).len() as u16;
        return (table + FOOTER_GAP).min(most);
    }
    fixed_height(app, body, headed).min(most)
}

/// tui.json's `card_height`: its rows, or its percent of the body and at least 8. By default 40%
/// of the body, and more when the list is short: the card takes the rows the list leaves empty,
/// so there is no blank band between them.
fn fixed_height(app: &App, body: Rect, headed: bool) -> u16 {
    let percent = match app.card_height {
        Some(CardHeight::Rows(rows)) => return rows,
        Some(CardHeight::Percent(percent)) => percent,
        None => 40,
    };
    let rows = u32::from(body.height) * u32::from(percent) / 100;
    let rows = u16::try_from(rows).unwrap_or(u16::MAX).max(8);
    if app.card_height.is_some() {
        return rows;
    }
    let items = app.listed(false);
    let groups = items
        .iter()
        .filter(|item| matches!(item, Item::Header { .. }))
        .count();
    // The list's rows and a blank one between it and the card.
    let need = items.len() + groups.saturating_sub(1) + usize::from(headed) + 1;
    let spare = usize::from(body.height).saturating_sub(need.max(1));
    rows.max(u16::try_from(spare).unwrap_or(u16::MAX))
}

/// While a prefix waits for its second key, which-key style: `g… g top · t next view …`, cut to
/// the footer. `o` lists the sort letters with their columns.
fn prefix_keys(app: &App, prefix: char, width: usize) -> Line<'static> {
    let pairs: Vec<(String, String)> = if prefix == 'o' {
        let fields = app.field_columns();
        app.builtin_columns()
            .chain(fields.iter().copied())
            .filter_map(|c| Some((c.letter?.to_string(), c.header.to_lowercase())))
            .collect()
    } else if prefix == 'x' {
        app.action_pairs()
    } else {
        keys::after(keys::BOARD, keys::K(KeyCode::Char(prefix), false))
            .into_iter()
            .map(|(k, word)| (k.to_string(), word.to_owned()))
            .collect()
    };
    let mut spans = vec![Span::styled(format!("{prefix}… "), theme::strong())];
    let mut used = prefix.len_utf8() + "… ".width();
    let tail = " · esc cancels";
    for (i, (key, word)) in pairs.iter().enumerate() {
        let sep = if i == 0 { "" } else { " · " };
        let piece = sep.width() + key.width() + 1 + word.width();
        if used + piece > width {
            break;
        }
        used += piece;
        spans.push(Span::styled(sep, theme::quiet()));
        spans.push(Span::styled(key.clone(), theme::strong()));
        spans.push(Span::styled(format!(" {word}"), theme::quiet()));
    }
    if used + tail.width() <= width {
        spans.push(Span::styled(tail, theme::quiet()));
    }
    Line::from(spans)
}

/// While `f` waits: each status with its count and its letter underlined, as the column header
/// marks sort letters. Narrower, the separators tighten and the hint goes, then `w4 d6 …`.
fn status_keys(app: &App, width: usize) -> Line<'static> {
    let named = |sep: &str, tail: &str| -> Line<'static> {
        let mut spans = vec![Span::styled("f ", theme::strong())];
        for (i, status) in ALL.iter().enumerate() {
            let (letter, rest) = status.name().split_at(1);
            if i > 0 {
                spans.push(Span::styled(sep.to_owned(), theme::quiet()));
            }
            spans.push(Span::styled(
                letter.to_owned(),
                theme::letter(theme::strong()),
            ));
            spans.push(Span::styled(
                format!("{rest} {}", app.status_count(*status)),
                theme::strong(),
            ));
        }
        spans.push(Span::styled(tail.to_owned(), theme::quiet()));
        Line::from(spans)
    };
    for line in [named(" · ", " · ff clears"), named("  ", "")] {
        if line.width() <= width {
            return line;
        }
    }
    let short: Vec<String> = ALL
        .iter()
        .map(|&s| format!("{}{}", s.letter(), app.status_count(s)))
        .collect();
    Line::styled(
        fit(&format!("f {} · ff clears", short.join(" ")), width),
        theme::strong(),
    )
}

/// `1 host`, `2 hosts`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// `view: table ↓age`, unless the board is on the status view with no sort.
fn view_label(app: &App) -> Option<String> {
    let only = app.status.map(|status| format!("only {}", status.name()));
    if app.view == View::default() && app.sort.is_none() {
        return only;
    }
    let sort = app.sort.map_or_else(String::new, |(id, dir)| {
        format!(" {}{}", dir.arrow(), column_of(id).header.to_lowercase())
    });
    let view = format!("view: {}{sort}", app.view.name);
    Some(match only {
        Some(only) => format!("{view} · {only}"),
        None => view,
    })
}

/// Hosts that are not up, then extensions whose last run failed: `mini down, ext fake failed`.
/// Failed extensions that would not fit after `rexboard · ` collapse to `2 ext failed`.
fn alerts(app: &App, width: usize) -> String {
    let mut alerts: Vec<String> = app
        .hosts
        .iter()
        .filter(|(_, state)| *state != "up")
        .map(|(host, state)| format!("{host} {state}"))
        .collect();
    let failed: Vec<&str> = app
        .ext_runs
        .iter()
        .filter(|(_, run)| !run.ok)
        .map(|(name, _)| name.as_str())
        .collect();
    if !failed.is_empty() {
        let named = format!("ext {} failed", failed.join(", "));
        let with = alerts.iter().map(String::as_str).chain([named.as_str()]);
        let room = width.saturating_sub("rexboard · ".width());
        alerts.push(if with.collect::<Vec<_>>().join(", ").width() <= room {
            named
        } else {
            format!("{} ext failed", failed.len())
        });
    }
    alerts.join(", ")
}

fn header(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let red = theme::alert();
    let (alert, alert_style) = match app.link {
        Link::Up => (alerts(app, usize::from(area.width)), red),
        Link::Connecting => ("connecting to serve".to_owned(), theme::quiet()),
        Link::Lost(_) => (STOPPED.to_owned(), red),
    };
    let counts = format!(
        " · {} · {} · {}",
        count(app.hosts.len(), "host"),
        count(app.panes, "pane"),
        count(app.rows.len(), "agent")
    );
    let healthy = app.hosts.keys().cloned().collect::<Vec<_>>().join(" + ");
    let label = view_label(app);
    let clock = now.with_timezone(&Local).format("%H:%M").to_string();
    let width = usize::from(area.width);
    // Alerts stay; the host list goes first, then the counts, then the clock.
    let shapes = [
        (true, true, true),
        (true, false, true),
        (false, false, true),
        (false, false, false),
    ];
    let (with_counts, with_hosts, with_clock) = shapes
        .into_iter()
        .find(|&(c, h, k)| {
            let left = 8
                + label.as_ref().map_or(0, |l| 3 + l.width())
                + if c { counts.width() } else { 0 }
                + if alert.is_empty() {
                    0
                } else {
                    3 + alert.width()
                };
            let right = if k { clock.width() } else { 0 }
                + if h && alert.is_empty() {
                    3 + healthy.width()
                } else {
                    0
                };
            // At least 3 columns before the clock, so the host list goes before it gets tight.
            left + right + 3 <= width
        })
        .unwrap_or((false, false, false));
    let mut left = vec![Span::styled("rexboard", theme::accent())];
    if let Some(label) = label {
        left.push(Span::styled(" · ", theme::quiet()));
        left.push(Span::styled(label, theme::strong()));
    }
    if with_counts {
        left.push(Span::styled(counts, theme::quiet()));
    }
    if !alert.is_empty() {
        left.push(Span::styled(" · ", theme::quiet()));
        left.push(Span::styled(alert, alert_style));
    }
    let mut right = Vec::new();
    if with_clock {
        right.push(Span::styled(clock, theme::quiet()));
    }
    if with_hosts && app.link == Link::Up && app.hosts.values().all(|s| s == "up") {
        right.push(Span::styled(format!(" · {healthy}"), theme::quiet()));
    }
    let used = |spans: &[Span]| spans.iter().map(|s| s.content.width()).sum::<usize>();
    let gap = width.saturating_sub(used(&left) + used(&right));
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The list, under a header line naming its columns. `board` is the whole screen.
fn list(frame: &mut Frame, area: Rect, app: &App, board: Rect, now: DateTime<Utc>) {
    let items = app.listed(false);
    if items.is_empty() {
        let width = usize::from(area.width);
        frame.render_widget(
            Paragraph::new(Line::styled(
                fit(&empty_message(app), width),
                theme::quiet(),
            )),
            area,
        );
        return;
    }
    let width = usize::from(area.width);
    let columns = app.columns(board.width, width);
    let area = if board.height >= HEADED {
        let [top, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        frame.render_widget(Paragraph::new(column_header(&columns, app.sort)), top);
        rest
    } else {
        area
    };
    let height = usize::from(area.height);
    // Blank rows part the groups only while every item still fits, so they never cost a row.
    let gaps = items
        .iter()
        .filter(|item| matches!(item, Item::Header { .. }))
        .count()
        .saturating_sub(1);
    app.spaced.set(gaps > 0 && items.len() + gaps <= height);
    let items = app.items();
    let room = if items.len() > height {
        height.saturating_sub(1)
    } else {
        height
    };
    let at = items.iter().position(|item| is_selected(app, item));
    let (_, kept, _) = app.drawn.get();
    let offset = scroll_offset(at, kept, items.len(), room);
    app.drawn.set((area.y, offset, room));
    let places = if columns.has(Col::Where) {
        wheres(
            &app.rows,
            columns.width(Col::Where).saturating_sub(1),
            columns.tier != Tier::Wide,
            // The host view's groups already name the host, so its rows need no `@HOST`.
            columns.has(Col::Host) || app.view.group_by == Some(Key::Host),
        )
    } else {
        blocks(
            &app.rows,
            columns.width(Col::Session).saturating_sub(1),
            columns.width(Col::Window).saturating_sub(1),
            columns.width(Col::Block).saturating_sub(1),
        )
    };
    let mut shown = 0;
    let mut lines: Vec<Line> = items
        .iter()
        .skip(offset)
        .take(room)
        .map(|item| match item {
            Item::Header {
                bucket,
                count,
                folded,
            } => group_header_line(
                bucket,
                *count,
                *folded,
                group_mark(app, bucket),
                is_selected(app, item),
                width,
            ),
            Item::Gap => Line::default(),
            &Item::Row(i) => row_line(
                &app.rows[i],
                {
                    shown += 1;
                    (shown <= NUMBERED).then_some(shown)
                },
                &places[i],
                app,
                &columns,
                is_selected(app, item),
                now,
            ),
        })
        .collect();
    if room < height {
        lines.push(Line::styled(
            format!(
                "{} {} more",
                theme::MORE,
                hidden_below(&items, offset + room)
            ),
            theme::quiet(),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// What an empty list says: why nothing matches, or how serve is.
fn empty_message(app: &App) -> String {
    let query = app.filter.value();
    if let Some(status) = app.status {
        if query.is_empty() {
            format!("no {} rows · ff or esc clears", status.name())
        } else {
            format!("no {} rows match \"{query}\" · esc clears", status.name())
        }
    } else if let Some(key) = app.unknown_key() {
        format!("no key {key} · esc clears")
    } else if !query.is_empty() {
        format!("no rows match \"{query}\" · esc clears")
    } else {
        match &app.link {
            Link::Up => "no agent panes yet".to_owned(),
            Link::Connecting => "waiting for serve".to_owned(),
            Link::Lost(why) => why.clone(),
        }
    }
}

/// The column header, each sort letter underlined.
fn column_header(columns: &Columns, sort: Option<Sort>) -> Line<'static> {
    let spans: Vec<Span> = columns
        .header(sort)
        .into_iter()
        .flat_map(|cell| {
            let Some(at) = cell.underline else {
                return vec![Span::styled(cell.text, theme::quiet())];
            };
            let (before, rest) = cell.text.split_at(at);
            let (letter, after) = rest.split_at(1);
            vec![
                Span::styled(before.to_owned(), theme::quiet()),
                Span::styled(letter.to_owned(), theme::letter(theme::quiet())),
                Span::styled(after.to_owned(), theme::quiet()),
            ]
        })
        .collect();
    Line::from(spans)
}

/// The first item shown. The list keeps its place while the selection is in view, so a clicked
/// row stays under the pointer; past the top it scrolls to put the selection first, past the
/// bottom last.
fn scroll_offset(at: Option<usize>, kept: usize, len: usize, room: usize) -> usize {
    let kept = kept.min(len.saturating_sub(room));
    match at {
        Some(at) if at < kept => at,
        Some(at) if at < kept + room => kept,
        Some(at) => (at + 1).saturating_sub(room),
        None => 0,
    }
}

/// In nerd mode, a harness group's mark before its name.
fn group_mark(app: &App, bucket: &Bucket) -> Option<&'static str> {
    match bucket {
        Bucket::Value(name) if app.nerd && app.view.group_by == Some(Key::Harness) => {
            crate::harness::mark(name).map(|icon| icon.glyph)
        }
        _ => None,
    }
}

fn group_header_line(
    bucket: &Bucket,
    count: usize,
    folded: bool,
    mark: Option<&str>,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let fold = if folded { theme::FOLDED } else { "" };
    let mark = mark.map(|m| format!("{m} ")).unwrap_or_default();
    let title = format!("{fold}{mark}{} ({count}) ", bucket.title());
    let fill = theme::GROUP_FILL.repeat(width.saturating_sub(title.width()));
    let line = Line::styled(format!("{title}{fill}"), theme::header());
    if selected {
        line.style(theme::selected(theme::header()))
    } else {
        line
    }
}

/// The rows past `from` that the list cannot show, counting a folded group's rows.
fn hidden_below(items: &[Item], from: usize) -> usize {
    items
        .iter()
        .skip(from)
        .map(|item| match item {
            Item::Row(_) => 1,
            Item::Header {
                count,
                folded: true,
                ..
            } => *count,
            Item::Header { .. } | Item::Gap => 0,
        })
        .sum()
}

fn is_selected(app: &App, item: &Item) -> bool {
    match (item, &app.selected) {
        (Item::Row(i), Some(Spot::Row(key))) => &app.rows[*i].key == key,
        (
            Item::Header {
                bucket,
                folded: true,
                ..
            },
            Some(Spot::Group(g)),
        ) => bucket == g,
        _ => false,
    }
}

/// One row of the list. `place` is its WHERE or BLOCK, worked out with the other rows.
fn row_line(
    row: &Row,
    number: Option<usize>,
    place: &str,
    app: &App,
    columns: &Columns,
    selected: bool,
    now: DateTime<Utc>,
) -> Line<'static> {
    let faded = row.stale || app.link != Link::Up;
    let soft = if row.pane.source == "inferred" || row.suspect || faded {
        theme::quiet()
    } else {
        theme::plain()
    };
    let mut spans: Vec<Span> = columns
        .cells(row, number, place, &app.marked(row).msg, now)
        .into_iter()
        .map(|(text, tone)| {
            let style = match tone {
                Tone::State => theme::state(row),
                Tone::Soft => soft,
                Tone::Dim => theme::quiet(),
                Tone::Fill(pct) => pct.and_then(theme::fill).unwrap_or(soft),
                Tone::Strong => soft.patch(theme::strong()),
            };
            Span::styled(text, style)
        })
        .collect();
    if faded {
        for span in &mut spans {
            span.style = span.style.patch(theme::quiet());
        }
    }
    if selected {
        for span in &mut spans {
            span.style = theme::selected(span.style);
        }
    }
    Line::from(spans)
}

/// What the footer says instead of its keys: a passing message, a running jump or why one failed.
pub(super) fn note(app: &App) -> Option<(&str, Style)> {
    app.toast()
        .map(|text| (text, theme::note()))
        .or_else(|| app.going.as_deref().map(|text| (text, theme::note())))
        .or_else(|| app.failed.as_deref().map(|text| (text, theme::alert())))
}

fn footer(frame: &mut Frame, area: Rect, app: &App) {
    let width = usize::from(area.width);
    let line = footer_overlay(app, width).unwrap_or_else(|| key_bar(app, width));
    frame.render_widget(Paragraph::new(line), area);
}

/// What takes the footer over, in order: the `:` and `/` prompts, a prefix waiting for its next
/// key, answer mode, `dd`'s question, then a note.
fn footer_overlay(app: &App, width: usize) -> Option<Line<'static>> {
    if app.commanding {
        let text = format!(
            ": {}{} · tab completes · esc cancels",
            app.command.value(),
            theme::CURSOR
        );
        return Some(Line::styled(fit(&text, width), theme::strong()));
    }
    if app.filtering {
        let text = format!("/ {}{}", app.filter.value(), theme::CURSOR);
        return Some(Line::styled(fit(&text, width), theme::strong()));
    }
    if let Some(line) = app.action_footer(width) {
        return Some(line);
    }
    if let Some((first, _)) = app.number {
        let text = format!("{first}… · a second digit or enter selects · esc cancels");
        return Some(Line::styled(fit(&text, width), theme::strong()));
    }
    match app.pending {
        Some('f') => return Some(status_keys(app, width)),
        Some(prefix) => return Some(prefix_keys(app, prefix, width)),
        None => {}
    }
    if let Some(answer) = &app.answer {
        return Some(answer.footer(width));
    }
    if let Some(question) = app.close_question(width) {
        return Some(Line::styled(fit(&question, width), theme::alert_label()));
    }
    note(app).map(|(text, style)| Line::styled(fit(text, width), style))
}

/// The keys, most needed first, with a kept `/` filter before them and quit at the right edge.
fn key_bar(app: &App, width: usize) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    let query = app.filter.value();
    if !query.is_empty() {
        let kept = fit(&format!("/{query}"), 16).trim_end().to_owned();
        used += kept.width() + " · esc clears · ".width();
        spans.push(Span::styled(kept, theme::strong()));
        spans.push(Span::styled(" · esc clears · ", theme::quiet()));
    }
    // `keys::FOOTER` ranks the keys; a narrow board drops them from the end.
    let keys: Vec<(&str, &str)> = if app.pick {
        vec![("enter", "go"), ("j/k", "move"), ("/", "filter")]
    } else {
        keys::FOOTER
            .iter()
            .copied()
            .filter(|&act| act != Act::CardDown || app.card_shown())
            .filter_map(|act| keys::entry(keys::BOARD, act))
            .map(|entry| (keys::short(entry), entry.word))
            .collect()
    };
    let (quit_key, quit_what) = if app.pick {
        ("esc", " cancel")
    } else {
        ("q", " quit")
    };
    let quit = format!("{quit_key}{quit_what}");
    let mut shown: Vec<(&str, &str)> = Vec::new();
    for (key, what) in keys {
        let sep = if shown.is_empty() { 0 } else { " · ".width() };
        let piece = sep + key.width() + 1 + what.width();
        if used + piece + quit.width() + 2 > width {
            break;
        }
        used += piece;
        shown.push((key, what));
    }
    for (i, (key, what)) in shown.into_iter().enumerate() {
        spans.push(Span::styled(
            if i == 0 { "" } else { " · " },
            theme::quiet(),
        ));
        spans.push(Span::styled(key, theme::strong()));
        spans.push(Span::styled(format!(" {what}"), theme::quiet()));
    }
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(used + quit.width()).max(2)),
    ));
    spans.push(Span::styled(quit_key, theme::strong()));
    spans.push(Span::styled(quit_what, theme::quiet()));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::tui::app::testkit::*;

    use std::time::{Duration, Instant};

    #[test]
    fn usage_shows_model_and_ctx_from_140_the_rest_when_asked_and_the_card_says_it() {
        use crate::harness::usage::Usage;
        let mut app = fuller();
        let plain = render(&app, 160, 30);
        assert!(
            !plain.contains("MODEL"),
            "no row reports usage, no columns:\n{plain}"
        );
        let key = app.current().unwrap().key.clone();
        let mut rows = app.rows.clone();
        for row in &mut rows {
            if row.key == key {
                row.usage = Some(Usage {
                    model: Some("Opus 5.5".into()),
                    ctx_pct: Some(88),
                    ctx_window: Some(200_000),
                    tok: Some(176_000),
                    cost_usd: Some(4.2),
                    quota_5h: Some(12),
                });
            }
        }
        app.set_rows(rows, app.hosts.clone());
        let wide = render(&app, 160, 30);
        let header = wide.lines().find(|l| l.contains("HARNESS")).unwrap();
        assert!(header.contains("MODEL") && header.contains("CTX"), "{wide}");
        for opt_in in ["TOK", "COST", "5H"] {
            assert!(
                !header.contains(opt_in),
                "{opt_in} waits for tui.json: {header}"
            );
        }
        let line = wide.lines().find(|l| l.contains("Opus 5.5")).expect(&wide);
        assert!(line.contains("88%") && !line.contains("176k"), "{line}");
        assert_eq!(
            header.find("CTX").unwrap() + 3,
            line.find("88%").unwrap() + 3,
            "figures are right-aligned under a right-aligned name:\n{header}\n{line}"
        );
        assert!(
            wide.lines()
                .filter(|l| l.contains("claude") && !l.contains("Opus"))
                .all(|l| l.contains(" -  ")),
            "a row with no usage shows -, not 0:\n{wide}"
        );
        let card = card_of(&wide);
        let fact = |card: &[String], label: &str, value: &str| {
            card.iter()
                .any(|l| l.contains(&format!("{label} ")) && l.contains(value))
        };
        assert!(fact(&card, "model", "Opus 5.5"), "{card:#?}");
        assert!(fact(&card, "ctx", "88% · 176k of 200k"), "{card:#?}");
        assert!(
            !card
                .iter()
                .any(|l| l.contains("cost") || l.contains("$4.20")),
            "{card:#?}"
        );
        assert!(
            !render(&app, 130, 30).lines().any(|l| l.contains("MODEL")),
            "below 140, no room"
        );
        app.columns = vec!["5h".into(), "cost".into(), "tok".into(), "later".into()];
        let opted = render(&app, 160, 30);
        let header = opted.lines().find(|l| l.contains("HARNESS")).unwrap();
        let at = |name: &str| header.find(name).unwrap();
        assert!(
            at("CTX") < at("TOK") && at("TOK") < at("COST") && at("COST") < at("5H"),
            "in the table's order, whatever tui.json's: {header}"
        );
        let line = opted.lines().find(|l| l.contains("Opus 5.5")).unwrap();
        assert!(
            line.contains("176k") && line.contains("$4.20") && line.contains("12%"),
            "{line}"
        );
        assert_eq!(
            at("COST") + 4,
            line.find("$4.20").unwrap() + 5,
            "{header}\n{line}"
        );
        let card = card_of(&opted);
        assert!(
            fact(&card, "cost", "$4.20") && fact(&card, "5h", "12%"),
            "{card:#?}"
        );
        let mut sorted = |sort: Sort| {
            app.sort = Some(sort);
            let text = render(&app, 160, 30);
            text.lines()
                .find(|l| l.contains("HARNESS"))
                .unwrap()
                .to_owned()
        };
        let header = sorted((Col::Ctx, crate::tui::columns::Dir::Down));
        assert!(header.contains(" ↓CTX "), "in its own room: {header}");
        let header = sorted((Col::Host, crate::tui::columns::Dir::Up));
        assert!(header.contains(" 5H HOST↑"), "never joins 5H: {header}");
        app.sort = None;
        let columns = Columns::with_usage(160, 160, &[], &app.usage_columns());
        let cells = columns.cells(app.current().unwrap(), None, "", &[], Utc::now());
        let tone = |want: &str| cells.iter().find(|(t, _)| t.trim() == want).unwrap().1;
        assert_eq!(tone("88%"), Tone::Fill(Some(88)));
        assert_eq!(
            tone("12%"),
            Tone::Fill(Some(12)),
            "5H fills by its own figure"
        );
    }

    #[test]
    fn model_and_ctx_sit_right_after_harness_with_the_other_usage_columns_after_ctx() {
        let mut app = fuller();
        let mut rows = app.rows.clone();
        rows[0].usage = Some(crate::harness::usage::Usage {
            model: Some("Opus 5.5".into()),
            ctx_pct: Some(40),
            tok: Some(1000),
            cost_usd: Some(1.5),
            quota_5h: Some(10),
            ..Default::default()
        });
        app.set_rows(rows, hosts());
        for (width, opted) in [(140, false), (200, false), (250, true), (200, true)] {
            app.columns = if opted {
                vec!["tok".into(), "cost".into(), "5h".into()]
            } else {
                Vec::new()
            };
            let text = render(&app, width, 30);
            let header = text.lines().find(|l| l.contains("HARNESS")).unwrap();
            let at = |name: &str| header.find(name).unwrap();
            let order = ["HARNESS", "MODEL", "CTX"];
            assert!(
                order.windows(2).all(|w| at(w[0]) < at(w[1])),
                "{width}: {header}"
            );
            let after = if opted { "5H" } else { "CTX" };
            assert!(
                at(after) < at("HOST") && at("HOST") < at("MSG"),
                "{width}: usage before HOST and MSG: {header}"
            );
            assert!(
                header[at("HARNESS")..at("MODEL")].trim_end().len() <= 10,
                "{width}: MODEL follows HARNESS at once: {header}"
            );
        }
    }

    #[test]
    fn below_160_usage_keeps_where_whole_so_msg_keeps_its_room_and_from_160_model_grows() {
        use crate::tui::columns::UsageShown;
        let usage = |model: usize| UsageShown {
            cols: vec![Col::Model, Col::Ctx],
            model,
        };
        let plain = Columns::new(140, 140, &[]);
        let with = Columns::with_usage(140, 140, &[], &usage(8));
        assert!(plain.has(Col::Session) && with.has(Col::Where) && !with.has(Col::Session));
        assert!(
            with.width(Col::Msg) >= plain.width(Col::Msg),
            "MSG at 140: {} with usage, {} without",
            with.width(Col::Msg),
            plain.width(Col::Msg)
        );
        assert!(Columns::with_usage(160, 160, &[], &usage(8)).has(Col::Session));
        let model = |board: u16, longest: usize| {
            Columns::with_usage(board, usize::from(board), &[], &usage(longest)).width(Col::Model)
        };
        assert_eq!(model(150, 17), 13, "below 160 MODEL keeps its least");
        assert_eq!(model(160, 8), 13);
        assert_eq!(model(160, 17), 18, "claude-sonnet-5-5 whole");
        assert_eq!(model(200, 40), 21, "and no more than 20");
    }

    #[test]
    fn a_context_warns_from_70_and_alerts_from_85() {
        assert_eq!(theme::fill(69), None);
        assert_eq!(
            theme::fill(70),
            Some(Style::new().fg(ratatui::style::Color::Yellow))
        );
        assert_eq!(theme::fill(85), Some(theme::alert()));
        assert_eq!(crate::output::tokens(950), "950");
        assert_eq!(crate::output::tokens(67_910), "68k");
        assert_eq!(crate::output::tokens(820_000), "820k");
        assert_eq!(crate::output::tokens(1_000_000), "1.0M");
        assert_eq!(crate::output::dollars(0.004), "$0.00");
        assert_eq!(crate::output::dollars(99.99), "$99.99");
        assert_eq!(crate::output::dollars(99.996), "$100");
        assert_eq!(crate::output::dollars(1234.5), "$1234");
    }

    use crate::tui::app::{App, Effect, Spot};

    #[test]
    fn renders_groups_counts_and_estimates_at_50_80_and_140() {
        for (width, height) in [(50, 30), (80, 30), (140, 40)] {
            let text = screen(width, height);
            for want in [
                "NEEDS YOU (2)",
                "WORKING (2)",
                "QUIET (1)",
                "UNREPORTED (1)",
                "≥5h43m",
                "rexboard",
            ] {
                assert!(
                    text.contains(want),
                    "{width}x{height} lacks {want:?}:\n{text}"
                );
            }
            assert!(text.contains("! "), "blocked glyph at {width}");
            assert!(!text.contains("NO SIGNAL"));
        }
    }

    #[test]
    fn columns_grow_with_the_width() {
        let narrow = screen(50, 30);
        let mid = screen(80, 30);
        let wide = screen(140, 40);
        let seat_line = |text: &str| {
            text.lines()
                .find(|l| l.contains("seat-b msg"))
                .unwrap()
                .to_owned()
        };
        assert!(!seat_line(&narrow).contains("claude"), "{narrow}");
        assert!(
            seat_line(&narrow).contains("api › seat-b"),
            "session › pane below 100:\n{narrow}"
        );
        assert!(seat_line(&mid).contains("claude"), "{mid}");
        assert!(seat_line(&mid).contains("mini"), "{mid}");
        assert!(seat_line(&wide).contains("blocked"), "{wide}");
        assert!(
            seat_line(&wide).contains(" api ")
                && seat_line(&wide).contains(" main ")
                && seat_line(&wide).contains(" seat-b "),
            "SESSION, WINDOW and BLOCK at 140:\n{wide}"
        );
        assert!(!wide.contains('│'), "no pane on the right at 140");
        assert!(
            wide.contains("━━ !") && narrow.contains("━━ !"),
            "the card is under the list at every width"
        );
        assert!(
            wide.lines().any(|l| l.contains("#l2 rig")),
            "a long window keeps its tail beside its block:\n{wide}"
        );
    }

    #[test]
    fn pick_ignores_c() {
        let mut app = fuller();
        app.pick = true;
        assert_eq!(press(&mut app, KeyCode::Char('c')), None);
        assert_eq!(press(&mut app, KeyCode::Char('p')), None);
        assert!((app.card, app.preview) == (true, false), "neither toggled");
    }

    #[test]
    fn a_row_with_no_record_shows_a_dash_for_its_age() {
        let wide = screen(140, 40);
        let line = wide.lines().find(|l| l.contains("inferred msg")).unwrap();
        assert!(line.contains(" -  "), "{line}");
    }

    #[test]
    fn a_folded_group_is_marked_and_says_how_to_open_it() {
        let mut app = fuller();
        press(&mut app, KeyCode::Char('z'));
        press(&mut app, KeyCode::Char('a'));
        let text = render(&app, 50, 30);
        assert!(text.contains("▸ NEEDS YOU (2)"), "{text}");
        assert!(text.contains("NEEDS YOU is folded · za opens it"), "{text}");
    }

    #[test]
    fn a_filter_with_no_match_says_so_and_a_kept_filter_shows() {
        let mut app = fuller();
        press(&mut app, KeyCode::Char('/'));
        for c in "zzz".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        let text = render(&app, 80, 30);
        assert!(
            text.contains("no rows match \"zzz\" · esc clears"),
            "{text}"
        );
        assert!(text.contains("/zzz · esc clears ·"), "{text}");
    }

    #[test]
    fn serve_stopped_stays_in_the_narrow_header() {
        let mut app = fuller();
        app.link = Link::Lost("serve stopped".into());
        let text = render(&app, 50, 30);
        assert!(
            text.lines()
                .next()
                .unwrap()
                .contains("serve stopped · R reconnects"),
            "{text}"
        );
        app.link = Link::Up;
        app.hosts.insert("mini".into(), "down".into());
        let text = render(&app, 50, 30);
        assert!(text.lines().next().unwrap().contains("mini down"), "{text}");
    }

    #[test]
    fn a_board_still_connecting_does_not_say_serve_stopped() {
        let mut app = App::default();
        let text = render(&app, 50, 30);
        let top = text.lines().next().unwrap();
        assert!(top.contains("connecting to serve"), "{text}");
        assert!(!text.contains("serve stopped"), "{text}");
        assert!(text.contains("waiting for serve"), "{text}");
        app.link = Link::Lost("serve: could not start it".into());
        let text = render(&app, 50, 30);
        assert!(
            text.lines().next().unwrap().contains("serve stopped"),
            "{text}"
        );
        assert!(
            text.contains("serve: could not start it"),
            "a failed start says why:\n{text}"
        );
    }

    #[test]
    fn dd_puts_its_question_in_the_footer_until_the_next_key() {
        let mut app = fuller();
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            render(&app, 80, 30).lines().last().unwrap(),
            "d… d close · esc cancels"
        );
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            render(&app, 80, 30).lines().last().unwrap(),
            "close api › main › seat-b on mini, ending claude (blocked)? y closes"
        );
        assert_eq!(
            render(&app, 60, 30).lines().last().unwrap(),
            "close api › main › seat-b on mini? y closes",
            "a narrow footer drops what it ends first"
        );
        assert_eq!(
            render(&app, 36, 30).lines().last().unwrap(),
            "close api › …eat-b on mini? y closes",
            "then cuts the place in the middle, never the key"
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            render(&app, 80, 30).lines().last().unwrap(),
            "nothing closed"
        );
    }

    /// A prefix lists the keys that may follow it, which-key style.
    #[test]
    fn a_pending_prefix_lists_its_next_keys() {
        let last = |keys: &str, width: u16| {
            let mut app = fuller();
            for c in keys.chars() {
                press(&mut app, KeyCode::Char(c));
            }
            render(&app, width, 30).lines().last().unwrap().to_owned()
        };
        assert_eq!(
            last("g", 140),
            "g… g top · t next view · T previous view · esc cancels"
        );
        assert_eq!(
            last("z", 140),
            "z… a toggle · o open · c fold · R open all · M fold all · esc cancels"
        );
        assert_eq!(last("y", 140), "y… y message · p place · esc cancels");
        assert_eq!(
            last("o", 140),
            "o… s state · a age · t turn · h harness · o host · w where · e session · n window · b block · m msg · esc cancels"
        );
        assert_eq!(last("o", 40), "o… s state · a age · t turn · h harness");
        assert!(!last("gx", 140).contains("g…"), "another key drops it");
    }

    /// `x` lists what applies to the row; an action's input line names its target and keeps the
    /// end being typed in view; its confirm line is the red one and always ends with the key.
    #[test]
    fn an_action_shows_its_menu_its_input_and_its_confirm_in_the_footer() {
        let last =
            |app: &App, width: u16| render(app, width, 30).lines().last().unwrap().to_owned();
        let mut app = with_actions();
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(last(&app, 100), "x… s send message · esc cancels");
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(
            last(&app, 100),
            "Send message api › main › seat-b on mini: ▏ · enter · esc"
        );
        typed(&mut app, "hello there, are you free");
        assert_eq!(
            last(&app, 100),
            "Send message api › main › seat-b on mini: hello there, are you free▏ · enter · esc"
        );
        let narrow = last(&app, 50);
        assert!(
            narrow.width() <= 50 && narrow.contains("you free▏"),
            "{narrow}"
        );
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            last(&app, 100),
            "send to api-lead on mini: \"hello there, are you free\" · it waits on a dialog · y runs"
        );
        let narrow = last(&app, 40);
        assert!(
            narrow.width() <= 40 && narrow.ends_with(" · y runs"),
            "{narrow}"
        );
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(last(&app, 80), "Send message cancelled");
        app.selected = Some(Spot::Row("local/seat-a".into()));
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(last(&app, 80), "no action for this row");
    }

    /// The confirm line of an action on a blocked row at `width`: the actual footer, after the
    /// keys, with the window that wide.
    fn confirm_at(width: u16, text: &str) -> (App, String) {
        let mut app = with_actions();
        app.width = width;
        typed(&mut app, &format!("xs{text}"));
        press(&mut app, KeyCode::Enter);
        let last = render(&app, width, 30).lines().last().unwrap().to_owned();
        (app, last)
    }

    /// At every width the confirm has the target and the whole short text, and the key; the dialog
    /// note gives way first (to its short form, then to nothing) and a long text is cut with the
    /// short note kept. Where no such line fits, nothing is armed.
    #[test]
    fn the_confirm_keeps_the_target_and_the_text_at_every_width_or_refuses_to_arm() {
        let full = "send to api-lead on mini: \"hi\" · it waits on a dialog · y runs";
        assert_eq!(confirm_at(80, "hi").1, full);
        for width in [36, 40, 45, 46, 60] {
            let (_, line) = confirm_at(width, "hi");
            assert!(line.width() <= usize::from(width), "{width}: {line}");
            assert!(
                line.contains(": \"hi\"") && line.ends_with(" · y runs"),
                "{width}: {line}"
            );
            assert!(
                line.starts_with("se"),
                "the target is there, cut: {width}: {line}"
            );
        }
        assert!(
            confirm_at(46, "hi").1.contains("dialog"),
            "the note stays while it fits"
        );
        assert!(
            !confirm_at(36, "hi").1.contains("waits"),
            "the full note gave way"
        );
        let long = "please look at the failing test in the parser and tell me what you think";
        let (_, line) = confirm_at(80, long);
        assert!(
            line.contains(" · dialog · y runs") && !line.contains("waits"),
            "{line}"
        );
        assert!(
            line.contains("\"please look at")
                && line.contains("what you think\"")
                && line.contains('…'),
            "{line}"
        );
        assert!(line.width() <= 80);
        // Too narrow for the target and the text: it says so and arms nothing.
        let (mut app, line) = confirm_at(22, "hi");
        assert_eq!(
            app.failed.as_deref(),
            Some("Send message cancelled: the screen is too narrow to confirm it")
        );
        assert!(
            line.starts_with("Send mess") && line.width() <= 22,
            "{line}"
        );
        assert!(press(&mut app, KeyCode::Char('y')).is_none());
        assert!(app.going.is_none());
        // A window that narrows after the confirm was drawn does not run it either.
        let (mut app, _) = confirm_at(80, "hi");
        app.width = 22;
        assert!(
            render(&app, 22, 30)
                .lines()
                .last()
                .unwrap()
                .ends_with("too narrow · esc"),
            "{:?}",
            render(&app, 22, 30).lines().last()
        );
        assert!(press(&mut app, KeyCode::Char('y')).is_none() && app.going.is_none());
        assert_eq!(
            app.failed.as_deref(),
            Some("Send message cancelled: the screen is too narrow to confirm it")
        );
    }

    #[test]
    fn the_narrow_key_bar_keeps_the_essentials_and_question_mark_shows_all() {
        let mut app = fuller();
        let text = render(&app, 50, 30);
        let bar = text.lines().last().unwrap();
        for want in ["j/k move", "enter jump", "q quit"] {
            assert!(bar.contains(want), "{bar}");
        }
        press(&mut app, KeyCode::Char('?'));
        // One column of 30 keys under the shortest list.
        let text = render(&app, 50, 40);
        for (key, _) in listed() {
            assert!(
                text.lines().any(|l| l.trim_start().starts_with(key)),
                "{key} missing:\n{text}"
            );
        }
    }

    #[test]
    fn a_suspect_row_says_it_may_be_stale() {
        let mut app = app();
        let mut rows = app.rows.clone();
        rows[0].suspect = true;
        rows[0].pane.state = "working".into();
        rows[0].pane.kind = None;
        rows[0].suspect_reason = Some("reported working at 12:27; the title shows idle".into());
        app.set_rows(rows, hosts());
        let text = render(&app, 80, 30);
        assert!(
            text.contains("may be stale: reported working at 12:27; the title shows idle"),
            "{text}"
        );
        assert!(text.contains("━━ ○ idle? · "), "{text}");
    }

    #[test]
    fn pick_shows_the_list_alone_with_its_own_keys() {
        let mut app = app();
        app.pick = true;
        for width in [50, 140] {
            let text = render(&app, width, 30);
            assert!(!text.contains("━━"), "no card:\n{text}");
            let bar = text.lines().last().unwrap();
            assert!(bar.starts_with("enter go · j/k move"), "{bar}");
            assert!(bar.ends_with("esc cancel"), "{bar}");
        }
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Quit));
    }

    #[test]
    fn at_100_columns_where_keeps_what_tells_rows_apart() {
        let mut app = App::default();
        app.card = true;
        let tab = "2-toolrig:rig:demo-team#0123456789abcdef#l2";
        let mut rows = Vec::new();
        for (key, session) in [("local/gw", "api-gateway"), ("local/fe", "web-frontend")] {
            let mut r = row(key, "blocked", "needs_you", false);
            (r.pane.session, r.pane.tab, r.pane.pane) =
                (session.into(), tab.into(), "seat-r".into());
            r.pane.window_id = format!("window:{session}");
            rows.push(r);
        }
        for key in ["local/block:b1", "local/block:b2"] {
            let mut r = row(key, "working", "working", true);
            (r.pane.session, r.pane.tab, r.pane.pane) =
                ("api".into(), "build".into(), "shell".into());
            r.pane.window_id = "window:build".into();
            rows.push(r);
        }
        app.set_rows(rows, hosts());
        // WHERE at its narrowest, 100 columns: the window gives way, the session and block stay.
        let text = render(&app, 100, 36);
        for want in [
            "api-gateway › 2-tool",
            "web-frontend › 2-too",
            "#l2 › seat-r",
            "api › build #1",
            "api › build #2",
        ] {
            assert!(text.contains(want), "{want:?} missing:\n{text}");
        }
        assert!(!text.contains(" … › … ›"), "{text}");
    }

    #[test]
    fn a_failed_jump_stays_in_the_footer_and_wraps_in_the_detail() {
        let mut app = app();
        app.going = Some("going to api › seat-b…".into());
        let text = render(&app, 80, 30);
        assert!(
            text.lines()
                .last()
                .unwrap()
                .starts_with("going to api › seat-b…"),
            "{text}"
        );
        app.went(Err(
            "the Rex app on studio did not answer within 5s; it does not answer while the \
             Mac's screen is locked"
                .into(),
        ));
        let text = render(&app, 80, 30);
        assert!(
            text.lines()
                .last()
                .unwrap()
                .starts_with("the Rex app on studio"),
            "{text}"
        );
        assert!(
            text.contains("screen is locked"),
            "wrapped whole in the detail:\n{text}"
        );
        assert!(!text.contains("jump failed: jump failed"), "{text}");
    }

    #[test]
    fn twins_stay_distinct_on_a_very_narrow_board() {
        let mut app = App::default();
        let mut rows = Vec::new();
        for (key, host) in [
            ("local/block:p1", "local"),
            ("local/block:p2", "local"),
            ("mini/block:p3", "mini"),
        ] {
            let mut r = row(key, "done", "needs_you", false);
            (r.pane.host, r.pane.session, r.pane.tab, r.pane.pane) = (
                host.into(),
                "api-gateway".into(),
                "pair".into(),
                "shell".into(),
            );
            r.pane.window_id = format!("window:pair-{host}");
            rows.push(r);
        }
        app.set_rows(rows, hosts());
        for width in [30, 40] {
            let text = render(&app, width, 20);
            let lines: Vec<&str> = text
                .lines()
                .filter(|l| l.get(3..).is_some_and(|r| r.starts_with('✓')))
                .collect();
            assert_eq!(lines.len(), 3, "{text}");
            assert!(lines.iter().any(|l| l.contains(" #1")), "{text}");
            assert!(lines.iter().any(|l| l.contains(" #2")), "{text}");
            assert!(lines.iter().any(|l| l.contains(" @mini")), "{text}");
            assert!(!text.contains("@local"), "{text}");
        }
    }

    #[test]
    fn the_header_counts_one_host_in_the_singular() {
        let mut app = App::default();
        let mut hosts = std::collections::BTreeMap::new();
        hosts.insert("local".to_owned(), "up".to_owned());
        app.set_rows(
            vec![row("local/block:one", "working", "working", true)],
            hosts,
        );
        let top = render(&app, 140, 10).lines().next().unwrap().to_owned();
        assert!(top.contains("1 host · 1 pane · 1 agent"), "{top}");
        let top = render(&self::app(), 140, 10)
            .lines()
            .next()
            .unwrap()
            .to_owned();
        assert!(top.contains("2 hosts · 4 panes · 4 agents"), "{top}");
    }

    #[test]
    fn rows_below_the_list_are_counted() {
        let text = render(&fuller(), 80, 14);
        assert!(text.contains("↓ "), "{text}");
        assert!(text.contains(" more"), "{text}");
    }

    #[test]
    fn every_view_renders_its_groups_under_one_header_at_50_and_120() {
        for (presses, want, never) in [
            ("", vec!["NEEDS YOU (2)", "WORKING (2)"], "CLAUDE"),
            ("v", vec!["seat-b msg"], "NEEDS YOU"),
            ("vv", vec!["CLAUDE (6)"], "NEEDS YOU"),
            ("vvv", vec!["LOCAL (3)", "MINI (3)"], "CLAUDE"),
        ] {
            for width in [50, 120] {
                let mut app = fuller();
                for key in presses.chars() {
                    press(&mut app, KeyCode::Char(key));
                }
                let text = render(&app, width, 30);
                let lines: Vec<&str> = text.lines().collect();
                assert_eq!(
                    lines[1].trim(),
                    "",
                    "{presses:?} {width}: the gap under the top bar"
                );
                assert!(lines[2].contains("AGE") || lines[2].contains("STATE"));
                for want in &want {
                    assert!(text.contains(want), "{presses:?} {width}: {want}\n{text}");
                }
                assert!(!text.contains(never), "{presses:?} {width}:\n{text}");
                assert!(
                    text.matches("seat-b msg").count() <= 2,
                    "each row once, and the card's headline\n{text}"
                );
            }
        }
    }

    #[test]
    fn the_top_bar_names_a_view_or_sort_that_is_not_the_default() {
        let mut app = fuller();
        assert!(
            !render(&app, 120, 30)
                .lines()
                .next()
                .unwrap()
                .contains("view:")
        );
        press(&mut app, KeyCode::Char('v'));
        crate::tui::app::testkit::sort(&mut app, 'a');
        let top = render(&app, 120, 30).lines().next().unwrap().to_owned();
        assert!(top.starts_with("rexboard · view: table ↓age · "), "{top}");
        let narrow = render(&app, 50, 30).lines().next().unwrap().to_owned();
        assert!(
            narrow.contains("view: table ↓age"),
            "it stays at 50: {narrow}"
        );
    }

    #[test]
    fn the_key_table_names_every_view_and_sort_letter() {
        for (key, what) in listed() {
            assert!(
                what.width() <= KEY_WIDTH,
                "{key}: {what:?} is cut in the key table"
            );
        }
        let mut app = fuller();
        press(&mut app, KeyCode::Char('?'));
        let text = render(&app, 120, 30);
        for want in [
            "v V      next, previous view (gt gT)",
            "/        filter: text or key:value",
            ":        a view, :seen or :q",
            "o        sort by a column's letter",
            "esc      cancel a key, clear filters",
            "f        one status; ff clears",
            "dd       close the pane; y confirms",
            "- ctrl-o back and forth, like cd -",
        ] {
            assert!(text.contains(want), "{want:?}:\n{text}");
        }
    }

    #[test]
    fn f_shows_each_status_with_its_count_until_its_letter() {
        let mut app = crate::tui::status::tests::every_status();
        press(&mut app, KeyCode::Char('f'));
        let last = |app: &App, width| {
            render(app, width, 30)
                .lines()
                .last()
                .unwrap()
                .trim_end()
                .to_owned()
        };
        assert_eq!(
            last(&app, 140),
            "f working 1 · done 2 · question 1 · permission 2 · error 1 · unreported 2 · idle 1 · ff clears"
        );
        assert_eq!(
            last(&app, 80),
            "f working 1  done 2  question 1  permission 2  error 1  unreported 2  idle 1"
        );
        assert_eq!(last(&app, 50), "f w1 d2 q1 p2 e1 u2 i1 · ff clears");
        press(&mut app, KeyCode::Char('p'));
        assert!(
            !last(&app, 140).starts_with("f w"),
            "the keys go once a letter is in"
        );
    }

    #[test]
    fn an_empty_status_filter_says_so_and_how_to_clear_it() {
        let mut app = crate::tui::status::tests::every_status();
        app.set_rows(
            app.rows
                .iter()
                .filter(|row| row.pane.state != "error")
                .cloned()
                .collect(),
            app.hosts.clone(),
        );
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('e'));
        assert!(
            render(&app, 80, 30).contains("no error rows · ff or esc clears"),
            "{}",
            render(&app, 80, 30)
        );
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Enter);
        assert!(render(&app, 80, 30).contains("no error rows match \"x\" · esc clears"));
    }

    #[test]
    fn the_top_bar_names_the_status_filter() {
        let mut app = crate::tui::status::tests::every_status();
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('q'));
        let top = render(&app, 120, 30).lines().next().unwrap().to_owned();
        assert!(top.starts_with("rexboard · only question · "), "{top}");
        for width in 30..130 {
            let top = render(&app, width, 30).lines().next().unwrap().to_owned();
            if let Some(at) = top.find(':').map(|i| i - 2) {
                assert!(
                    top[..at].ends_with("   "),
                    "{width}: at least 3 spaces before the clock\n{top}"
                );
            }
        }
        press(&mut app, KeyCode::Char('v'));
        crate::tui::app::testkit::sort(&mut app, 'a');
        let top = render(&app, 120, 30).lines().next().unwrap().to_owned();
        assert!(
            top.starts_with("rexboard · view: table ↓age · only question · "),
            "{top}"
        );
    }

    #[test]
    fn a_board_down_to_3_columns_draws_without_a_panic() {
        for width in [3, 9, 10, 11] {
            let text = render(&fuller(), width, 14);
            assert_eq!(text.lines().count(), 14, "{width}");
        }
    }

    #[test]
    fn the_footer_lists_the_keys_in_ranked_order_and_a_narrow_board_drops_from_the_end() {
        let app = fuller();
        let bar = |width: u16| {
            let text = render(&app, width, 30);
            text.lines().last().unwrap().to_owned()
        };
        let at_80 = bar(80);
        assert!(
            at_80.starts_with(
                "j/k move · enter jump · space full · p preview · ? keys · n needs you "
            ) && at_80.trim_end().ends_with(" q quit"),
            "{at_80}"
        );
        assert!(
            bar(200).starts_with(
                "j/k move · enter jump · space full · p preview · ? keys · n needs you · / filter · m seen · a answer · c card · v next view · 1-99 select"
            ),
            "{}",
            bar(200)
        );
        for width in [50, 64, 100, 140] {
            let bar = bar(width);
            assert!(bar.starts_with("j/k move · enter jump"), "{width}: {bar}");
            assert!(
                !bar.contains("scroll card"),
                "{width}: no card, no hint: {bar}"
            );
        }
    }

    #[test]
    fn groups_part_with_a_blank_row_only_while_every_row_fits() {
        let line_above = |text: &str, header: &str| {
            let lines: Vec<&str> = text.lines().collect();
            let at = lines.iter().position(|l| l.starts_with(header)).unwrap();
            lines[at - 1].to_owned()
        };
        let mut app = fuller();
        let roomy = render(&app, 80, 30);
        assert!(app.spaced.get());
        for header in ["WORKING (2)", "QUIET (1)", "UNREPORTED (1)"] {
            assert_eq!(line_above(&roomy, header), "", "{header}:\n{roomy}");
        }
        assert!(
            line_above(&roomy, "NEEDS YOU (2)").contains("HARNESS"),
            "none above the first:\n{roomy}"
        );
        assert_eq!(app.spots().len(), 6, "a blank row is never a spot");
        let mut seen = Vec::new();
        for _ in 0..7 {
            press(&mut app, KeyCode::Char('j'));
            seen.push(app.selected.clone().unwrap());
        }
        assert_eq!(seen[0], seen[6], "j walks the six rows and wraps");
        let (first, ..) = app.drawn.get();
        let gap = roomy
            .lines()
            .position(|l| l.starts_with("WORKING"))
            .unwrap()
            - 1;
        let before = app.selected.clone();
        app.click(5, gap as u16);
        assert_eq!(
            app.selected, before,
            "a click on a blank row selects nothing"
        );
        assert!(gap as u16 > first);

        press(&mut app, KeyCode::Char('c'));
        let tight = render(&app, 80, 14);
        assert!(!app.spaced.get(), "{tight}");
        assert!(
            line_above(&tight, "WORKING (2)").contains("claude"),
            "no blank rows once the list scrolls:\n{tight}"
        );
    }

    #[test]
    fn a_view_grouped_by_a_column_gives_that_column_to_msg() {
        let header_and_msg = |view: &str| {
            let app = harnesses(view);
            let text = render(&app, 140, 30);
            let header = text
                .lines()
                .nth(usize::from(HEADER_ROW))
                .unwrap()
                .to_owned();
            (header, app.columns(140, 140).width(Col::Msg))
        };
        let (status, msg) = header_and_msg("status");
        assert!(
            status.contains("HARNESS") && status.contains("HOST"),
            "{status}"
        );
        let (harness, harness_msg) = header_and_msg("harness");
        assert!(
            !harness.contains("HARNESS") && harness.contains("HOST"),
            "{harness}"
        );
        // MSG is at its floor here, so the freed column goes to MSG less the wider part gaps it now
        // allows.
        assert!(harness_msg >= msg + 7, "{msg} {harness_msg}");
        let (host, host_msg) = header_and_msg("host");
        assert!(host.contains("HARNESS") && !host.contains("HOST"), "{host}");
        assert!(host_msg >= msg + 5, "{msg} {host_msg}");
        let text = render(&harnesses("harness"), 140, 30);
        for group in [
            "CLAUDE (2)",
            "CODEX (1)",
            "GROK (1)",
            "OPENCODE (1)",
            "PI (1)",
        ] {
            assert!(text.contains(group), "{group}:\n{text}");
        }
    }

    #[test]
    fn harness_marks_show_only_in_nerd_mode_and_only_for_an_adapter_with_one() {
        let claude = "\u{ec82}";
        let mut status = harnesses("status");
        let words = render(&status, 140, 30);
        assert!(!words.contains(claude), "{words}");
        status.nerd = true;
        let marked = render(&status, 140, 30);
        assert!(marked.contains(&format!("{claude} claude")), "{marked}");
        assert!(marked.contains("\u{ec81} codex"), "{marked}");
        let column = |msg: &str, word: &str| {
            let line = marked.lines().find(|l| l.contains(msg)).unwrap();
            line.chars()
                .collect::<String>()
                .find(word)
                .map(|at| line[..at].chars().count())
        };
        assert_eq!(
            column("seat-a msg", "grok"),
            column("claude msg", "claude"),
            "grok keeps its word, under the marked ones:\n{marked}"
        );
        let mut grouped = harnesses("harness");
        grouped.nerd = true;
        let text = render(&grouped, 140, 30);
        assert!(text.contains(&format!("{claude} CLAUDE (2)")), "{text}");
        assert!(text.contains("\u{e22c} PI (1)"), "{text}");
        assert!(text.lines().any(|l| l.starts_with("GROK (1)")), "{text}");
    }

    #[test]
    fn the_selection_stays_put_when_a_group_empties_or_appears() {
        let mut app = fuller();
        app.selected = Some(Spot::Row("local/seat-a".into()));
        render(&app, 80, 30);
        let working = |row: &Row| row.group == crate::board::Group::Working;
        let rows: Vec<Row> = app.rows.iter().filter(|r| !working(r)).cloned().collect();
        let all = app.rows.clone();
        app.set_rows(rows, hosts());
        let text = render(&app, 80, 30);
        assert!(!text.contains("WORKING"), "{text}");
        assert_eq!(app.selected, Some(Spot::Row("local/seat-a".into())));
        app.set_rows(all, hosts());
        let text = render(&app, 80, 14);
        assert_eq!(app.selected, Some(Spot::Row("local/seat-a".into())));
        assert!(text.contains("seat-a msg"), "still on screen:\n{text}");
    }

    #[test]
    fn the_host_view_drops_the_at_host_its_group_already_names() {
        let mut app = App::default();
        let mut rows = Vec::new();
        for (key, host) in [("local/block:p1", "local"), ("mini/block:p3", "mini")] {
            let mut r = row(key, "done", "needs_you", false);
            (r.pane.host, r.pane.session, r.pane.tab, r.pane.pane) = (
                host.into(),
                "api-gateway".into(),
                "pair".into(),
                "shell".into(),
            );
            r.pane.window_id = format!("window:pair-{host}");
            rows.push(r);
        }
        app.set_rows(rows, hosts());
        assert!(render(&app, 50, 20).contains("@mini"), "status keeps it");
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('v'));
        }
        assert_eq!(app.view.name, "host");
        let text = render(&app, 50, 20);
        assert!(
            text.contains("MINI (1)") && !text.contains("@mini"),
            "{text}"
        );
    }

    #[test]
    fn the_key_table_sits_a_blank_row_above_the_footer_and_the_list_keeps_the_rest() {
        let mut app = fuller();
        press(&mut app, KeyCode::Char('?'));
        for (width, height) in [(80u16, 30u16), (140, 60), (200, 60)] {
            let text = render(&app, width, height);
            let lines: Vec<&str> = text.lines().collect();
            let title = lines
                .iter()
                .position(|l| l.starts_with(" keys"))
                .expect(&text);
            let table = &lines[title + 1..lines.len() - 2];
            assert!(
                !table.is_empty() && table.iter().all(|l| !l.trim().is_empty()),
                "{width}x{height}: the table runs down to the row above the footer:\n{text}"
            );
            assert_eq!(lines[lines.len() - 2], "", "{width}x{height}:\n{text}");
            assert_eq!(
                list_rows(&text.replace(" keys", "━━")),
                title - 3,
                "{width}x{height}: the list has every row above it"
            );
        }
    }

    #[test]
    fn c_hides_the_card_and_keeps_the_choice() {
        let mut app = fuller();
        let effect = press(&mut app, KeyCode::Char('c'));
        assert!(matches!(effect, Some(Effect::SavePrefs(prefs)) if prefs.card == Some(false)));
        let text = render(&app, 140, 30);
        assert!(!text.contains("━━"), "{text}");
        assert!(text.lines().last().unwrap().contains("c card"), "{text}");
        press(&mut app, KeyCode::Char('?'));
        assert!(
            render(&app, 140, 30).contains(" keys"),
            "the key table still shows"
        );
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('c'));
        assert!(render(&app, 140, 30).contains("━━ !"));
        let mut picking = fuller();
        picking.pick = true;
        assert_eq!(
            press(&mut picking, KeyCode::Char('c')),
            None,
            "pick has no card"
        );
    }

    #[test]
    fn tab_goes_round_the_rows_that_need_you_and_marks_a_read_done_row_seen() {
        let mut app = app();
        let long_ago = Instant::now() - Duration::from_secs(2);
        app.shown = Some(("mini/seat-b".into(), long_ago));
        assert_eq!(
            press(&mut app, KeyCode::Tab),
            None,
            "a blocked row is never marked"
        );
        assert_eq!(app.current().unwrap().key, "local/claude");
        app.shown = Some(("local/claude".into(), Instant::now()));
        assert_eq!(press(&mut app, KeyCode::Tab), None, "not read for 1s yet");
        assert_eq!(
            app.current().unwrap().key,
            "mini/seat-b",
            "wraps to the first"
        );
        press(&mut app, KeyCode::Tab);
        app.shown = Some(("local/claude".into(), long_ago));
        let effect = press(&mut app, KeyCode::Tab);
        assert!(
            matches!(effect, Some(Effect::Seen { ref key, .. }) if key == "local/claude"),
            "{effect:?}"
        );
    }

    #[test]
    fn a_blocked_row_tails_itself_after_300ms_once_and_a_sweep_tails_none() {
        let mut board = app();
        let t0 = Instant::now();
        board.note_card(t0);
        assert_eq!(board.auto_tail(t0 + Duration::from_millis(100)), None);
        let effect = board.auto_tail(t0 + Duration::from_millis(301));
        assert!(
            matches!(effect, Some(Effect::Peek { ref key, .. }) if key == "mini/seat-b"),
            "{effect:?}"
        );
        assert_eq!(
            board.auto_tail(t0 + Duration::from_secs(2)),
            None,
            "once per report"
        );
        let mut sweep = fuller();
        let mut t = Instant::now();
        for _ in 0..5 {
            press(&mut sweep, KeyCode::Char('j'));
            t += Duration::from_millis(50);
            sweep.note_card(t);
            assert_eq!(sweep.auto_tail(t), None);
        }
        let mut done = app();
        done.selected = Some(Spot::Row("local/claude".into()));
        done.note_card(t0);
        assert_eq!(
            done.auto_tail(t0 + Duration::from_secs(1)),
            None,
            "only blocked rows"
        );
    }

    #[test]
    fn digits_f_and_sort_letters_keep_their_meaning() {
        let mut app = app();
        crate::tui::app::testkit::render(&app, 80, 30);
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.current().unwrap().key, "local/claude");
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('c'));
        assert!(app.card, "c after f belongs to f");
        assert_eq!(app.toast(), Some("f takes w d q p e u i; ff clears"));
        crate::tui::app::testkit::sort(&mut app, 's');
        assert!(app.sort.is_some(), "o s sorts");
    }

    /// Every cell's symbol and style, one line per row, the style written where it changes.
    fn styled_dump(app: &App, width: u16, height: u16) -> String {
        let now = DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app, now)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            let mut last = None;
            for x in 0..width {
                let cell = &buffer[(x, y)];
                let style = (cell.fg, cell.bg, cell.modifier);
                if last != Some(style) {
                    out.push_str(&format!("{{{:?}/{:?}/{:?}}}", style.0, style.1, style.2));
                    last = Some(style);
                }
                out.push_str(cell.symbol());
            }
            out.push('\n');
        }
        out
    }

    /// The board's looks across its screens, cell by cell: a change of look shows as a diff of
    /// the fixture, recorded with UPDATE_STYLE_SNAPSHOT=1.
    #[test]
    fn every_screen_matches_the_style_snapshot() {
        let mut screens: Vec<(&str, App, u16, u16)> = vec![
            ("status 80", fuller(), 80, 30),
            ("status 140", fuller(), 140, 34),
            ("every state 80", every_state(), 80, 32),
            ("every state 140", every_state(), 140, 32),
            ("every state 200", every_state(), 200, 32),
        ];
        let mut table = fuller();
        press(&mut table, KeyCode::Char('v'));
        crate::tui::app::testkit::sort(&mut table, 'a');
        screens.push(("table sorted 140", table, 140, 30));
        let mut keys = fuller();
        press(&mut keys, KeyCode::Char('?'));
        screens.push(("keys 120", keys, 120, 34));
        for (name, width, height) in [
            ("keys 80", 80, 30),
            ("keys 140 tall", 140, 60),
            ("keys 200 tall", 200, 60),
        ] {
            let mut keys = fuller();
            press(&mut keys, KeyCode::Char('?'));
            screens.push((name, keys, width, height));
        }
        for (name, width, height) in [
            ("done card 80", 80, 30),
            ("done card 200 tall", 200, 60),
            ("done card nerd 140", 140, 30),
        ] {
            let mut done = fuller();
            let mut rows = done.rows.clone();
            let row = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
            row.last_turn = Some(372);
            row.pane.msg = Some("Opened PR #31; CI is green. ⏎ Next step is yours.".into());
            done.set_rows(rows, hosts());
            done.selected = Some(Spot::Row("local/claude".into()));
            done.nerd = name.contains("nerd");
            screens.push((name, done, width, height));
        }
        for view in ["harness", "host"] {
            for width in [140, 200] {
                for nerd in [false, true] {
                    let mut app = harnesses(view);
                    app.nerd = nerd;
                    let name = format!("{view} view{} {width}", if nerd { " nerd" } else { "" });
                    screens.push((name.leak(), app, width, 40));
                }
            }
        }
        for (name, width) in [("markdown 80", 80), ("markdown 140", 140)] {
            let mut marked = fuller();
            let mut rows = marked.rows.clone();
            let row = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
            row.pane.msg = Some(
                "## Summary ⏎ **Tests pass**: ran `cargo test` on [the PR](https://x.test/31) ⏎ \
                 - next: **merge** it, then `make release` ⏎ ```sh ⏎ make release ⏎ ``` ⏎ snake_case and 2*3*4 stay"
                    .into(),
            );
            marked.set_rows(rows, hosts());
            marked.selected = Some(Spot::Row("local/claude".into()));
            screens.push((name, marked, width, 30));
        }
        let mut laid = fuller();
        let mut rows = laid.rows.clone();
        let row = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
        row.pane.msg = Some(
            [
                "## Release **0.16**",
                "",
                "The card keeps one blank line between paragraphs, and this one is long enough to wrap \
                 across the card's width at 140 columns.",
                "",
                "",
                "- a list item long enough to wrap, whose second row hangs under its text and not under \
                 the marker that leads it",
                "12. a numbered one, which hangs four cells in",
                "> a quote long enough to wrap keeps its marker on every row it takes, so it reads as a \
                 quote all the way down",
                "```rust",
                "    let total: u64 = rows.iter().map(|row| row.tokens).sum(); // a code line long enough to reach the card's edge",
                "```",
                "| crate | before | after |",
                "|---|:-:|---|",
                "| **rexboard** | 0.15.0 | 0.16.0 |",
            ]
            .join(" ⏎ "),
        );
        laid.set_rows(rows, hosts());
        laid.selected = Some(Spot::Row("local/claude".into()));
        let mut narrow = App::default();
        narrow.set_rows(laid.rows.clone(), hosts());
        narrow.card = true;
        narrow.selected = laid.selected.clone();
        screens.push(("card layout 80", narrow, 80, 48));
        screens.push(("card layout 140", laid, 140, 44));
        let mut status = fuller();
        press(&mut status, KeyCode::Char('f'));
        screens.push(("f picker 80", status, 80, 20));
        let mut filter = fuller();
        press(&mut filter, KeyCode::Char('/'));
        for c in "seat".chars() {
            press(&mut filter, KeyCode::Char(c));
        }
        screens.push(("filter prompt 80", filter, 80, 20));
        let mut command = fuller();
        press(&mut command, KeyCode::Char(':'));
        screens.push(("command prompt 80", command, 80, 20));
        let mut hidden = fuller();
        press(&mut hidden, KeyCode::Char('c'));
        hidden.toast = None;
        screens.push(("card hidden 80", hidden, 80, 24));
        let mut failed = fuller();
        failed.failed = Some("that pane is gone".into());
        failed.hosts.insert("studio".into(), "down".into());
        screens.push(("failed jump and a host down 80", failed, 80, 30));
        let mut toast = fuller();
        toast.say("a passing message");
        screens.push(("toast 80", toast, 80, 20));
        let mut prefix = fuller();
        press(&mut prefix, KeyCode::Char('z'));
        screens.push(("z prefix 140", prefix, 140, 20));
        let mut close = fuller();
        press(&mut close, KeyCode::Char('d'));
        press(&mut close, KeyCode::Char('d'));
        screens.push(("dd question 140", close, 140, 20));
        let pinned = |mut app: App| {
            let at = DateTime::parse_from_rfc3339("2026-01-01T05:42:58Z").unwrap();
            app.answer.as_mut().unwrap().captured = Some(at.with_timezone(&Utc));
            app
        };
        screens.push(("answer 80", pinned(answer_mode_on(PERMISSION)), 80, 30));
        let mut refused = pinned(answer_mode_on(PERMISSION));
        refused.answer.as_mut().unwrap().error = Some("the screen changed; nothing sent".into());
        screens.push(("answer refused 80", refused, 80, 30));
        let long = format!("\x1b[48;2;0;64;0m+ {}\x1b[0m", "x".repeat(120));
        screens.push((
            "full coloured 80",
            full_of(vec![
                "\x1b[31m- removed\x1b[0m".into(),
                long,
                "plain".into(),
            ]),
            80,
            12,
        ));
        let mut failing = full_of(numbered(3));
        failing.peeked("mini/seat-b", Err("rex timed out".into()));
        screens.push(("full failing 80", failing, 80, 10));
        let mut text = String::new();
        for (name, app, width, height) in &mut screens {
            // A toast lasts 3 s from its `say`, and under load the screens built and drawn before
            // this one can take that long. An instant in the future never expires.
            if let Some((_, at)) = app.toast.as_mut() {
                *at = Instant::now() + Duration::from_secs(3600);
            }
            text.push_str(&format!("== {name}\n{}", styled_dump(app, *width, *height)));
        }
        // The clock and a capture time show in the local zone, which differs between machines.
        let local = |at: &str, format: &str| {
            let at = DateTime::parse_from_rfc3339(at).unwrap();
            at.with_timezone(&chrono::Local).format(format).to_string()
        };
        let text = text
            .replace(&local("2026-01-01T05:42:58Z", "%H:%M:%S"), "hh:mm:ss")
            .replace(&local("2026-01-01T05:43:00Z", "%H:%M"), "hh:mm");
        let path = format!(
            "{}/tests/fixtures/style-snapshot.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        if std::env::var_os("UPDATE_STYLE_SNAPSHOT").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let want = std::fs::read_to_string(&path).expect("the snapshot");
        for ((name, ..), (got, want)) in screens
            .iter()
            .zip(text.split("== ").skip(1).zip(want.split("== ").skip(1)))
        {
            assert_eq!(got, want, "{name} changed");
        }
        assert_eq!(text, want);
    }
}

#[cfg(test)]
mod extension_tests {
    use std::collections::BTreeMap;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::client::{ExtRun, Field};
    use crate::tui::app::App;
    use crate::tui::app::testkit::listed;
    use crate::tui::app::testkit::{app, hosts, press};
    use crate::tui::columns::{Col, Columns, Dir};

    fn field(id: &str, title: &str) -> Field {
        Field {
            id: id.into(),
            name: id.split_once('.').unwrap().1.into(),
            title: title.into(),
            ..Field::default()
        }
    }

    /// Two extensions: `fake` gives `rig`, `session-prefix` gives `project`; one row has neither.
    fn with_fields(extra: &[Field]) -> App {
        let mut app = app();
        let mut rows = app.rows.clone();
        for (key, rig, project) in [
            ("mini/seat-b", Some("api"), Some("api")),
            ("local/claude", Some("web"), None),
            ("mini/codex", Some("api"), Some("gateway")),
            ("local/seat-a", None, None),
        ] {
            let row = rows.iter_mut().find(|r| r.key == key).unwrap();
            row.ext
                .extend(rig.map(|v| ("fake.rig".to_owned(), v.to_owned())));
            row.ext
                .extend(project.map(|v| ("session-prefix.project".to_owned(), v.to_owned())));
        }
        let mut fields = vec![
            field("fake.rig", "RIG"),
            field("session-prefix.project", "PROJECT"),
        ];
        fields.extend_from_slice(extra);
        app.set_extensions(fields, BTreeMap::new());
        app.set_rows(rows, hosts());
        app
    }

    const WIDE_MSG: &str = "Opened the pull request and queued the review for the api team today";

    /// `with_fields` with a third extension field, a long host name, and window labels as an
    /// orchestrator generates them.
    fn wide_board() -> App {
        let mut app = with_fields(&[field("fake.role", "ROLE")]);
        let mut rows = app.rows.clone();
        for row in &mut rows {
            row.ext.insert("fake.role".into(), "seat".into());
            row.pane.tab = format!("1-team:rig:{}#0123456789abcdef#l6", row.pane.session);
            row.pane.msg = Some(WIDE_MSG.into());
        }
        rows[0].pane.host = "build-host-01".into();
        app.set_rows(rows, hosts());
        app
    }

    fn header(app: &App, width: u16) -> String {
        render(app, width, 30).lines().nth(2).unwrap().to_owned()
    }

    #[test]
    fn host_shows_the_longest_host_name_whole_up_to_16_and_80_columns_stay() {
        let app = wide_board();
        for width in [140u16, 200, 250] {
            let text = render(&app, width, 30);
            assert!(
                text.contains("build-host-01 "),
                "{width}: HOST whole:\n{text}"
            );
            let columns = app.columns(width, usize::from(width));
            assert_eq!(columns.width(Col::Host), 14, "{width}: 13 and a gap");
        }
        let mut long = wide_board();
        let mut rows = long.rows.clone();
        rows[0].pane.host = "h".repeat(30);
        long.set_rows(rows, hosts());
        assert_eq!(
            long.columns(200, 200).width(Col::Host),
            16,
            "never wider than 16"
        );
        let plain = app_default();
        assert_eq!(
            plain.columns(80, 80).width(Col::Host),
            8,
            "short names keep 8"
        );
        assert_eq!(
            plain
                .columns(80, 80)
                .cols
                .iter()
                .map(|(_, w)| *w)
                .collect::<Vec<_>>(),
            Columns::new(80, 80, &[])
                .cols
                .iter()
                .map(|(_, w)| *w)
                .collect::<Vec<_>>(),
            "80 columns do not change"
        );
    }

    fn app_default() -> App {
        let mut app = App::default();
        let mut rows = crate::tui::app::testkit::rows();
        for row in &mut rows {
            row.pane.tab = "main".into();
        }
        app.set_rows(rows, hosts());
        app
    }

    #[test]
    fn msg_keeps_a_floor_and_labels_their_cap_with_three_extension_columns() {
        let app = wide_board();
        for width in [140u16, 200, 250] {
            let columns = app.columns(width, usize::from(width));
            let floor = 40
                .max(usize::from(width) * 35 / 100)
                .min(WIDE_MSG.len() + 1);
            assert!(
                columns.width(Col::Msg) >= floor,
                "{width}: MSG is {} wide, under its floor of {floor}:\n{}",
                columns.width(Col::Msg),
                header(&app, width)
            );
            for part in [Col::Session, Col::Window, Col::Block] {
                assert!(
                    columns.width(part) <= 27,
                    "{width}: {part:?} past the 24 cap"
                );
            }
        }
        let at = |width| {
            let header = header(&app, width);
            ["RIG", "PROJECT", "ROLE"].map(|name| header.contains(name))
        };
        assert_eq!(at(250), [true, true, true], "room for all three at 250");
        assert!(
            !at(140)[2],
            "the extension columns give way once the parts are at their least: {}",
            header(&app, 140)
        );
    }

    /// `wide_board` with the first `fields` extension fields, MODEL and CTX (`usage` 1) or all
    /// the usage columns (2), and Nerd marks or not.
    fn configured(fields: usize, usage: u8, nerd: bool) -> App {
        let mut app = wide_board();
        let all = ["fake.rig", "session-prefix.project", "fake.role"];
        let titles = ["RIG", "PROJECT", "ROLE"];
        app.set_extensions(
            (0..fields).map(|i| field(all[i], titles[i])).collect(),
            BTreeMap::new(),
        );
        let mut rows = app.rows.clone();
        for row in &mut rows {
            row.usage = (usage > 0).then(|| crate::harness::usage::Usage {
                model: Some("claude-model".into()),
                ctx_pct: Some(40),
                tok: Some(1000),
                cost_usd: Some(0.5),
                quota_5h: Some(10),
                ..Default::default()
            });
        }
        app.set_rows(rows, hosts());
        if usage > 1 {
            app.columns = vec!["tok".into(), "cost".into(), "5h".into()];
        }
        app.nerd = nerd;
        app
    }

    #[test]
    fn msg_keeps_its_floor_with_usage_icons_and_extension_columns_at_every_width() {
        for width in [140u16, 200, 250, 1000] {
            let floor = 40
                .max(usize::from(width) * 35 / 100)
                .min(WIDE_MSG.len() + 1);
            for fields in 0..=3 {
                for usage in 0..=2 {
                    for nerd in [false, true] {
                        let app = configured(fields, usage, nerd);
                        let columns = app.columns(width, usize::from(width));
                        let sum: usize = columns.cols.iter().map(|(_, w)| *w).sum();
                        let case =
                            format!("{width} cols, {fields} fields, usage {usage}, nerd {nerd}");
                        assert!(sum <= usize::from(width), "{case}: {sum} cells");
                        assert!(
                            columns.width(Col::Msg) >= floor,
                            "{case}: MSG is {}, under {floor}:\n{}",
                            columns.width(Col::Msg),
                            header(&app, width)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_fixed_width_leaves_the_automatic_parts_to_give_way_and_a_fixed_msg_ends_the_floor() {
        let mut app = wide_board();
        app.widths = BTreeMap::from([("session".to_owned(), 90.into())]);
        let columns = app.columns(250, 250);
        assert_eq!(columns.width(Col::Session), 90);
        assert!(
            columns.width(Col::Msg) > WIDE_MSG.len(),
            "the other parts gave way, not MSG: {}",
            header(&app, 250)
        );
        let mut msg_one = wide_board();
        msg_one.widths = BTreeMap::from([("msg".to_owned(), 1.into())]);
        let columns = msg_one.columns(140, 140);
        assert_eq!(columns.width(Col::Msg), 1);
        let text = header(&msg_one, 140);
        assert!(
            text.contains("RIG") && text.contains("PROJECT"),
            "a fixed msg keeps the extension columns: {text}"
        );
        let mut grow_last = configured(0, 0, false);
        grow_last.widths = BTreeMap::from([
            ("session".to_owned(), 180.into()),
            ("window".to_owned(), 1.into()),
            ("block".to_owned(), 1.into()),
            ("msg".to_owned(), 500.into()),
        ]);
        let columns = grow_last.columns(250, 250);
        assert_eq!(
            [Col::Session, Col::Window, Col::Block].map(|c| columns.width(c)),
            [180, 1, 1],
            "the shrinks come back before the long one grows"
        );
        let mut too_much = wide_board();
        too_much.widths = BTreeMap::from([
            ("session".to_owned(), 300.into()),
            ("window".to_owned(), 300.into()),
        ]);
        let columns = too_much.columns(140, 140);
        let sum: usize = columns.cols.iter().map(|(_, w)| *w).sum();
        assert!(
            sum <= 140 && columns.width(Col::Msg) >= 5,
            "{sum}: {}",
            header(&too_much, 140)
        );
    }

    #[test]
    fn host_is_planned_with_the_parts_and_fixed_parts_still_leave_msg_its_floor() {
        let mut app = configured(0, 0, false);
        app.widths = BTreeMap::from([
            ("host".to_owned(), 180.into()),
            ("session".to_owned(), 1.into()),
            ("window".to_owned(), 1.into()),
            ("block".to_owned(), 1.into()),
            ("msg".to_owned(), 500.into()),
        ]);
        let columns = app.columns(250, 250);
        assert_eq!(
            [Col::Host, Col::Session, Col::Window, Col::Block].map(|c| columns.width(c)),
            [180, 1, 1, 1],
            "HOST grows once the parts have shrunk: {}",
            header(&app, 250)
        );
        // Every part fixed, no `msg`: the extension columns still give way to the floor.
        let mut fixed = wide_board();
        let mut rows = fixed.rows.clone();
        for row in &mut rows {
            row.pane.msg = Some("long message ".repeat(30));
        }
        fixed.set_rows(rows, hosts());
        fixed.widths = BTreeMap::from([
            ("session".to_owned(), 87.into()),
            ("window".to_owned(), 10.into()),
            ("block".to_owned(), 10.into()),
        ]);
        let columns = fixed.columns(250, 250);
        assert!(
            columns.width(Col::Msg) >= 250 * 35 / 100,
            "MSG is {}: {}",
            columns.width(Col::Msg),
            header(&fixed, 250)
        );
        assert!(!header(&fixed, 250).contains("ROLE"), "a field gave way");
    }

    #[test]
    fn short_messages_do_not_cost_a_label_its_cut() {
        let mut app = wide_board();
        let mut rows = app.rows.clone();
        for row in &mut rows {
            row.pane.msg = Some("Done.".into());
            row.pane.pane = "seat-with-a-name-20c".into();
        }
        app.set_rows(rows, hosts());
        for width in [200u16, 250] {
            let text = render(&app, width, 30);
            assert!(
                text.lines()
                    .any(|l| l.contains("seat-with-a-name-20c ") && l.contains("Done.")),
                "{width}: the block shows whole:\n{text}"
            );
        }
    }

    #[test]
    fn tui_json_widths_fix_a_column_at_every_width_and_msg_floor_waits_for_msg() {
        let mut app = wide_board();
        app.widths = std::collections::BTreeMap::from([
            ("host".to_owned(), 12.into()),
            ("session".to_owned(), 20.into()),
            ("window".to_owned(), 21.into()),
            ("block".to_owned(), 18.into()),
            ("msg".to_owned(), 60.into()),
            ("nonsense".to_owned(), 3.into()),
            ("bad".to_owned(), "wide".into()),
        ]);
        for width in [140u16, 200, 250] {
            let columns = app.columns(width, usize::from(width));
            let got = [Col::Host, Col::Session, Col::Window, Col::Block].map(|c| columns.width(c));
            assert_eq!(got, [12, 20, 21, 18], "{width}: {}", header(&app, width));
            let msg = columns.width(Col::Msg);
            let others: usize = columns.cols.iter().map(|(_, w)| *w).sum::<usize>() - msg;
            assert_eq!(
                msg,
                60.min(usize::from(width) - others),
                "{width}: 60, or what the others leave"
            );
        }
        assert_eq!(
            app.columns(80, 80).width(Col::Host),
            12,
            "HOST shows at 80 too"
        );
        let mut only_session = wide_board();
        only_session.widths = std::collections::BTreeMap::from([("session".to_owned(), 30.into())]);
        let columns = only_session.columns(250, 250);
        assert_eq!(columns.width(Col::Session), 30);
        assert!(
            columns.width(Col::Msg) >= 250 * 35 / 100,
            "the floor holds with msg unset"
        );
    }

    fn render(app: &App, width: u16, height: u16) -> String {
        let now = DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app, now)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn header_row(app: &App, width: u16) -> String {
        render(app, width, 30)
            .lines()
            .nth(usize::from(HEADER_ROW))
            .unwrap()
            .to_owned()
    }

    fn keys(app: &App) -> Vec<String> {
        app.items()
            .into_iter()
            .filter_map(|item| match item {
                crate::tui::app::Item::Row(i) => Some(app.rows[i].key.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn v_reaches_a_view_per_field_grouped_by_value_with_none_last() {
        let mut app = with_fields(&[]);
        let names: Vec<String> = app.views().into_iter().map(|v| v.name).collect();
        assert_eq!(
            names,
            ["status", "table", "harness", "host", "rig", "project"]
        );
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('v'));
        }
        assert_eq!(app.view.name, "rig");
        let text = render(&app, 120, 30);
        let groups: Vec<&str> = text
            .lines()
            .filter(|l| l.contains(" (") && l.contains("┄"))
            .map(|l| l.split(" ┄").next().unwrap())
            .collect();
        assert_eq!(groups, ["API (2)", "WEB (1)", "- (1)"], "{text}");
    }

    #[test]
    fn field_columns_follow_host_from_120_titled_from_the_manifest() {
        let app = with_fields(&[field("third.owner", "OWNER")]);
        let at = |width| header_row(&app, width);
        let words = |line: String| {
            line.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert!(!at(119).contains("RIG"), "{}", at(119));
        let mid = words(at(140));
        let host = mid.iter().position(|w| w.starts_with("HOST")).unwrap();
        assert_eq!(&mid[host + 1..host + 3], ["RIG", "PROJECT"], "{mid:?}");
        assert!(
            !mid.contains(&"OWNER".to_owned()),
            "only two below 160: {mid:?}"
        );
        assert!(words(at(160)).contains(&"OWNER".to_owned()), "{}", at(160));
        let text = render(&app, 140, 30);
        let seat_b = text.lines().find(|l| l.contains("seat-b")).unwrap();
        assert!(seat_b.contains(" api "), "{seat_b}");
        let seat_a = text.lines().find(|l| l.contains("seat-a")).unwrap();
        assert!(seat_a.contains(" -  "), "no value is -: {seat_a}");
    }

    #[test]
    fn a_fields_column_hides_in_its_own_view_and_sorts_by_its_free_letter() {
        let mut app = with_fields(&[]);
        let cells = Columns::new(140, 140, &app.field_columns()).header(None);
        let rig = cells.iter().find(|c| c.text.starts_with("RIG")).unwrap();
        assert_eq!(rig.underline, Some(0), "r, since HARNESS sorts by h");
        let project = cells
            .iter()
            .find(|c| c.text.starts_with("PROJECT"))
            .unwrap();
        assert_eq!(project.underline, Some(0));
        press(&mut app, KeyCode::Char('v'));
        crate::tui::app::testkit::sort(&mut app, 'r');
        assert_eq!(app.sort, Some((Col::Field("fake.rig"), Dir::Up)));
        assert_eq!(
            keys(&app),
            ["mini/seat-b", "mini/codex", "local/claude", "local/seat-a"],
            "api, api, web, then none"
        );
        app.view = app.views().into_iter().find(|v| v.name == "rig").unwrap();
        let header = header_row(&app, 140);
        assert!(
            !header.contains("RIG") && header.contains("PROJECT"),
            "{header}"
        );
    }

    #[test]
    fn keyed_filter_terms_match_one_key_each_and_and_together() {
        let mut app = with_fields(&[]);
        let mut filtered = |query: &str| {
            app.filter = query.to_owned().into();
            keys(&app)
        };
        assert_eq!(filtered("rig:api state:blocked"), ["mini/seat-b"]);
        assert_eq!(filtered("fake.rig:API"), ["mini/seat-b", "mini/codex"]);
        assert_eq!(filtered("project:gate"), ["mini/codex"]);
        assert_eq!(
            filtered("rig:web claude"),
            ["local/claude"],
            "free text too"
        );
        assert_eq!(
            filtered("host:local window:main"),
            ["local/claude", "local/seat-a"]
        );
        assert!(filtered("rig:nope").is_empty());
        assert!(filtered("project:api").len() == 1, "no value never matches");
        assert!(
            filtered("nope:x").is_empty(),
            "an unknown key is plain text"
        );
    }

    #[test]
    fn a_failed_extension_is_a_red_alert_that_collapses_when_narrow() {
        let mut app = with_fields(&[]);
        let failed = ExtRun {
            ok: false,
            error: Some("timed out".into()),
        };
        app.ext_runs = BTreeMap::from([("fake".to_owned(), failed.clone())]);
        let top = |app: &App, width| render(app, width, 30).lines().next().unwrap().to_owned();
        assert!(
            top(&app, 50).starts_with("rexboard · ext fake failed"),
            "{}",
            top(&app, 50)
        );
        assert!(
            top(&app, 120).contains(" · 4 agents · ext fake failed "),
            "{}",
            top(&app, 120)
        );
        app.ext_runs.insert("session-prefix".into(), failed);
        app.hosts.insert("studio".into(), "down".into());
        assert!(
            top(&app, 120).contains("studio down, ext fake, session-prefix failed"),
            "{}",
            top(&app, 120)
        );
        assert!(
            top(&app, 50).starts_with("rexboard · studio down, 2 ext failed"),
            "{}",
            top(&app, 50)
        );
    }

    #[test]
    fn the_key_table_gets_a_row_for_the_fields_only_while_there_are_some() {
        let mut app = with_fields(&[field("lane.lane", "LANE")]);
        press(&mut app, KeyCode::Char('?'));
        let text = render(&app, 120, 30);
        assert!(
            text.lines()
                .any(|l| l.contains("o r p l  sort by rig, project, lane")),
            "the names alone when the sources would be cut:\n{text}"
        );
        let rows = key_rows(&app);
        let at = rows.iter().position(|(k, _)| k == "o r p l").unwrap();
        assert_eq!(rows[at - 1].0, "o", "right after the sort row");
        let mut one = crate::tui::app::testkit::app();
        one.set_extensions(vec![field("fake.rig", "RIG")], BTreeMap::new());
        assert!(
            key_rows(&one).contains(&("o r".to_owned(), "sort by rig (fake)".to_owned())),
            "the source while it fits"
        );
        let mut plain = crate::tui::app::testkit::app();
        press(&mut plain, KeyCode::Char('?'));
        assert_eq!(
            key_rows(&plain).len(),
            listed().len(),
            "no row without extensions"
        );
    }

    #[test]
    fn a_filter_with_a_key_nothing_has_says_so() {
        let mut app = with_fields(&[]);
        app.filter = "projct:api".to_owned().into();
        assert!(render(&app, 80, 30).contains("no key projct · esc clears"));
        app.filter = "project:zzz".to_owned().into();
        assert!(render(&app, 80, 30).contains("no rows match \"project:zzz\" · esc clears"));
    }

    #[test]
    fn the_card_lists_each_extensions_fields_as_facts_named_after_the_field() {
        let mut app = with_fields(&[]);
        app.selected = Some(crate::tui::app::Spot::Row("mini/seat-b".into()));
        let text = render(&app, 80, 30);
        let lines: Vec<&str> = text.lines().collect();
        let at = lines
            .iter()
            .position(|l| l.starts_with(" rig "))
            .expect(&text);
        assert_eq!(
            lines[at], " rig     api                    project api",
            "each field is a track of its own: no extension name, no run-on line"
        );
        assert!(
            !text.contains("session-prefix") && !text.contains("fake "),
            "{text}"
        );
        let mut lone = with_fields(&[field("lane.lane", "LANE")]);
        let mut rows = lone.rows.clone();
        rows[0].ext.insert("lane.lane".into(), "desk".into());
        lone.set_rows(rows, hosts());
        lone.selected = Some(crate::tui::app::Spot::Row(lone.rows[0].key.clone()));
        let text = render(&lone, 80, 30);
        assert!(
            text.lines().any(|l| l.starts_with(" lane    desk")),
            "a field named like its extension is one fact: {text}"
        );
        app.nerd = true;
        let text = render(&app, 80, 30);
        assert!(
            text.lines().any(|l| l.starts_with(" rig     api")),
            "a field with no declared mark keeps its name, and the card keeps the words' grid: {text}"
        );
        for (field, _) in &mut app.fields {
            field.icon = Some("\u{f502}".into());
        }
        let text = render(&app, 80, 30);
        assert!(
            text.lines().any(|l| l.starts_with(" \u{f502} api")),
            "its own mark leads, one cell: {text}"
        );
    }

    #[test]
    fn fields_that_go_take_their_view_and_sort_with_them_and_none_change_nothing() {
        let mut app = with_fields(&[]);
        app.view = app.views().into_iter().find(|v| v.name == "rig").unwrap();
        press(&mut app, KeyCode::Char('P'));
        app.set_extensions(Vec::new(), BTreeMap::new());
        assert_eq!(app.view.name, "status");
        assert_eq!(app.sort, None);
        let mut plain = crate::tui::app::testkit::app();
        let before: Vec<String> = [50, 80, 120, 140, 200]
            .map(|w| render(&plain, w, 30))
            .into();
        plain.set_extensions(Vec::new(), BTreeMap::new());
        let after: Vec<String> = [50, 80, 120, 140, 200]
            .map(|w| render(&plain, w, 30))
            .into();
        assert_eq!(before, after, "with no extensions the board is as it was");
    }
}
