use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Item, Link, Spot};
use super::columns::{Col, Columns, Tier, Tone, column_of, tier};
use super::status::ALL;
use super::theme;
use super::views::{Key, View};
use crate::board::Row;
use crate::output::{blocks, fit, wheres};

/// Below this many rows the column header and the blank row above it are left out, so no row
/// is lost to them.
pub(super) const HEADED: u16 = 12;
/// The column header's row when there is one: under the top bar and its blank row.
pub(super) const HEADER_ROW: u16 = 2;
/// The list rows the card always leaves.
const LIST_FLOOR: u16 = 3;

const STOPPED: &str = "serve stopped · r reconnects";

/// The `?` table: every key, in the order an operator reaches for them.
pub const KEYS: [(&str, &str); 22] = [
    ("j/k", "move; g/G first, last"),
    ("1-9", "the nth NEEDS YOU row"),
    ("s", "mark seen"),
    ("ctrl+s", "mark done rows seen (was S)"),
    ("space", "full view of the pane, live"),
    ("tab", "the next row that needs you"),
    ("/", "filter; enter keeps it"),
    ("f", "one status: w d q p e u i"),
    ("v", "status, table, harness, host"),
    (":", "a view by name; tab completes"),
    ("S A T", "sort by state, age, turn"),
    ("R O W M", "by harness, host, where, msg"),
    ("E N B", "by session, window, block"),
    ("click", "a header sorts, in table"),
    ("esc", "clear filters/sort/peek/folds"),
    ("z", "fold or open a group"),
    ("enter", "go to the pane"),
    ("a", "answer: pick an option, reply"),
    ("r", "reconnect to serve"),
    ("c", "hide or show the card"),
    ("?", "this table"),
    ("q", "quit"),
];

pub fn draw(frame: &mut Frame, app: &App, now: DateTime<Utc>) {
    if let Some(full) = &app.full
        && let Some(row) = app.rows.iter().find(|row| row.key == full.key)
    {
        return super::full::draw(frame, app, full, row, now);
    }
    let area = frame.area();
    let card = app.card_shown();
    let ruled = !card && tier(area.width) != Tier::Wide && area.height >= 20;
    let [top, _gap, body, above_footer, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(area.height >= HEADED)),
        Constraint::Min(3),
        Constraint::Length(u16::from(ruled)),
        Constraint::Length(1),
    ])
    .areas(area);
    header(frame, top, app, now);
    let height = if card {
        card_height(app, now, body, area.height >= HEADED)
    } else {
        0
    };
    let [list_area, card_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(body);
    if list_area.height > 0 {
        list(frame, list_area, app, area, now);
    }
    if height > 0 {
        let lines = super::card::lines(app, now, usize::from(body.width), usize::from(height));
        frame.render_widget(Paragraph::new(lines), card_area);
    }
    frame.render_widget(rule_line(above_footer.width), above_footer);
    footer(frame, bottom, app);
}

/// The card's rows: at least 8 and 40% of the body, more for the key table
/// or a blocked row's ask, which is never cut, but always leaving the list 3 rows under its column
/// header, when it has one.
fn card_height(app: &App, now: DateTime<Utc>, body: Rect, headed: bool) -> u16 {
    let most = body.height.saturating_sub(LIST_FLOOR + u16::from(headed));
    if let Some(answer) = &app.answer {
        let lines = |rows: u16| {
            super::card::lines(app, now, usize::from(body.width), usize::from(rows)).len() as u16
        };
        let mut height = (body.height * 2 / 5).max(8).max(lines(most)).min(most);
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
    let base = (body.height * 2 / 5).max(8);
    let asks = app.keys || app.current().is_some_and(|row| row.pane.state == "blocked");
    let need = if asks {
        super::card::lines(app, now, usize::from(body.width), 0).len() as u16
    } else {
        0
    };
    base.max(need).min(most)
}

/// While `f` waits: each status with its count and its letter underlined, as the column header
/// marks sort letters. Narrower, the separators tighten and the hint goes, then `w4 d6 …`.
fn status_keys(app: &App, width: usize) -> Line<'static> {
    let named = |sep: &str, tail: &str| -> Line<'static> {
        let mut spans = vec![Span::styled("f ", theme::bright())];
        for (i, status) in ALL.iter().enumerate() {
            let (letter, rest) = status.name().split_at(1);
            if i > 0 {
                spans.push(Span::styled(sep.to_owned(), theme::dim()));
            }
            spans.push(Span::styled(
                letter.to_owned(),
                theme::bright().add_modifier(Modifier::UNDERLINED),
            ));
            spans.push(Span::styled(
                format!("{rest} {}", app.status_count(*status)),
                theme::bright(),
            ));
        }
        spans.push(Span::styled(tail.to_owned(), theme::dim()));
        Line::from(spans)
    };
    for line in [named(" · ", " · f/esc clears"), named("  ", "")] {
        if line.width() <= width {
            return line;
        }
    }
    let short: Vec<String> = ALL
        .iter()
        .map(|&s| format!("{}{}", s.letter(), app.status_count(s)))
        .collect();
    Line::styled(
        fit(&format!("f {} · f/esc clears", short.join(" ")), width),
        theme::bright(),
    )
}

fn rule_line(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(theme::dim())
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

fn header(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let red = Style::new().fg(Color::Red);
    let (alert, alert_style) = match app.link {
        Link::Up => (
            app.hosts
                .iter()
                .filter(|(_, state)| *state != "up")
                .map(|(host, state)| format!("{host} {state}"))
                .collect::<Vec<_>>()
                .join(", "),
            red,
        ),
        Link::Connecting => ("connecting to serve".to_owned(), theme::dim()),
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
        left.push(Span::styled(" · ", theme::dim()));
        left.push(Span::styled(label, theme::bright()));
    }
    if with_counts {
        left.push(Span::styled(counts, theme::dim()));
    }
    if !alert.is_empty() {
        left.push(Span::styled(" · ", theme::dim()));
        left.push(Span::styled(alert, alert_style));
    }
    let mut right = Vec::new();
    if with_clock {
        right.push(Span::styled(clock, theme::dim()));
    }
    if with_hosts && app.link == Link::Up && app.hosts.values().all(|s| s == "up") {
        right.push(Span::styled(format!(" · {healthy}"), theme::dim()));
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
    let items = app.items();
    if items.is_empty() {
        let query = app.filter.value();
        let text = if let Some(status) = app.status {
            if query.is_empty() {
                format!("no {} rows · f f or esc clears", status.name())
            } else {
                format!("no {} rows match \"{query}\" · esc clears", status.name())
            }
        } else if !query.is_empty() {
            format!("no rows match \"{query}\" · esc clears")
        } else {
            match &app.link {
                Link::Up => "no agent panes yet".to_owned(),
                Link::Connecting => "waiting for serve".to_owned(),
                Link::Lost(why) => why.clone(),
            }
        };
        let width = usize::from(area.width);
        frame.render_widget(
            Paragraph::new(Line::styled(fit(&text, width), theme::dim())),
            area,
        );
        return;
    }
    let width = usize::from(area.width);
    let columns = Columns::new(board.width, width);
    let area = if board.height >= HEADED {
        let [top, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        let spans: Vec<Span> = columns
            .header(app.sort)
            .into_iter()
            .flat_map(|cell| {
                let Some(at) = cell.underline else {
                    return vec![Span::styled(cell.text, theme::dim())];
                };
                let (before, rest) = cell.text.split_at(at);
                let (letter, after) = rest.split_at(1);
                vec![
                    Span::styled(before.to_owned(), theme::dim()),
                    Span::styled(
                        letter.to_owned(),
                        theme::dim().add_modifier(Modifier::UNDERLINED),
                    ),
                    Span::styled(after.to_owned(), theme::dim()),
                ]
            })
            .collect();
        frame.render_widget(Paragraph::new(Line::from(spans)), top);
        rest
    } else {
        area
    };
    let height = usize::from(area.height);
    let room = if items.len() > height {
        height.saturating_sub(1)
    } else {
        height
    };
    let at = items.iter().position(|item| is_selected(app, item));
    // The list keeps its place while the selection is in view, so a clicked row stays under the
    // pointer; past the top it scrolls to put the selection first, past the bottom last.
    let (_, kept, _) = app.drawn.get();
    let kept = kept.min(items.len().saturating_sub(room));
    let offset = match at {
        Some(at) if at < kept => at,
        Some(at) if at < kept + room => kept,
        Some(at) => (at + 1).saturating_sub(room),
        None => 0,
    };
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
    let mut lines: Vec<Line> = items
        .iter()
        .skip(offset)
        .take(room)
        .map(|item| match item {
            Item::Header {
                bucket,
                count,
                folded,
            } => {
                let mark = if *folded { "▸ " } else { "" };
                let title = format!("{mark}{} ({count}) ", bucket.title());
                let fill = "┄".repeat(width.saturating_sub(title.width()));
                let line = Line::styled(format!("{title}{fill}"), theme::header());
                if is_selected(app, item) {
                    line.style(theme::header().bg(theme::RAISED))
                } else {
                    line
                }
            }
            &Item::Row(i) => row_line(
                &app.rows[i],
                &places[i],
                app,
                &columns,
                is_selected(app, item),
                now,
            ),
        })
        .collect();
    let below: usize = items
        .iter()
        .skip(offset + room)
        .map(|item| match item {
            Item::Row(_) => 1,
            Item::Header {
                count,
                folded: true,
                ..
            } => *count,
            Item::Header { .. } => 0,
        })
        .sum();
    if room < height {
        lines.push(Line::styled(format!("↓ {below} more"), theme::dim()));
    }
    frame.render_widget(Paragraph::new(lines), area);
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
    place: &str,
    app: &App,
    columns: &Columns,
    selected: bool,
    now: DateTime<Utc>,
) -> Line<'static> {
    let faded = row.stale || app.link != Link::Up;
    let soft = if row.pane.source == "inferred" || row.suspect || faded {
        theme::dim()
    } else {
        Style::new()
    };
    let mut spans: Vec<Span> = columns
        .cells(row, place, now)
        .into_iter()
        .map(|(text, tone)| {
            let style = match tone {
                Tone::State => theme::state(row),
                Tone::Soft => soft,
                Tone::Dim => theme::dim(),
            };
            Span::styled(text, style)
        })
        .collect();
    if faded {
        for span in &mut spans {
            span.style = span.style.patch(theme::dim());
        }
    }
    if selected {
        for span in &mut spans {
            span.style = span.style.bg(theme::RAISED);
        }
    }
    Line::from(spans)
}

/// What the footer says instead of its keys: a passing message, a running jump or why one failed.
pub(super) fn note(app: &App) -> Option<(&str, Style)> {
    let accent = Style::new().fg(theme::ACCENT);
    app.toast()
        .map(|text| (text, accent))
        .or_else(|| app.going.as_deref().map(|text| (text, accent)))
        .or_else(|| {
            app.failed
                .as_deref()
                .map(|text| (text, Style::new().fg(Color::Red)))
        })
}

fn footer(frame: &mut Frame, area: Rect, app: &App) {
    let width = usize::from(area.width);
    if app.commanding {
        let text = format!(": {}▏ · tab completes · esc cancels", app.command.value());
        frame.render_widget(
            Paragraph::new(Line::styled(fit(&text, width), theme::bright())),
            area,
        );
        return;
    }
    if app.filtering {
        let text = format!("/ {}▏", app.filter.value());
        frame.render_widget(
            Paragraph::new(Line::styled(fit(&text, width), theme::bright())),
            area,
        );
        return;
    }
    if app.picking_status {
        frame.render_widget(Paragraph::new(status_keys(app, width)), area);
        return;
    }
    if let Some(answer) = &app.answer {
        frame.render_widget(Paragraph::new(answer.footer(width)), area);
        return;
    }
    if let Some((text, style)) = note(app) {
        frame.render_widget(Paragraph::new(Line::styled(fit(text, width), style)), area);
        return;
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    let query = app.filter.value();
    if !query.is_empty() {
        let kept = fit(&format!("/{query}"), 16).trim_end().to_owned();
        used += kept.width() + " · esc clears · ".width();
        spans.push(Span::styled(kept, theme::bright()));
        spans.push(Span::styled(" · esc clears · ", theme::dim()));
    }
    // The essentials first, so a narrow board keeps them; then `tab next`, which works through
    // the rows that need you, and `v view` last, one `?` away. `v view` shows after `/ filter`.
    // While the card is hidden, `c card` says so, and `space full` shows after it when it fits.
    let keys: Vec<(&str, &str)> = if app.pick {
        vec![("enter", "go"), ("j/k", "move"), ("/", "filter")]
    } else {
        let mut keys = vec![
            ("j/k", "move"),
            ("s", "seen"),
            ("/", "filter"),
            ("?", "keys"),
        ];
        if app.hidden {
            keys.push(("c", "card"));
        }
        keys.extend([
            ("tab", "next"),
            ("enter", "jump"),
            ("a", "answer"),
            ("space", "full"),
            ("v", "view"),
            ("1-9", "select"),
        ]);
        keys
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
    for (moved, after) in [("v", "/"), ("space", "c")] {
        if let Some(at) = shown.iter().position(|&(key, _)| key == moved) {
            let piece = shown.remove(at);
            let to = shown
                .iter()
                .position(|&(key, _)| key == after)
                .map_or(at, |i| i + 1);
            shown.insert(to, piece);
        }
    }
    for (i, (key, what)) in shown.into_iter().enumerate() {
        spans.push(Span::styled(if i == 0 { "" } else { " · " }, theme::dim()));
        spans.push(Span::styled(key, theme::bright()));
        spans.push(Span::styled(format!(" {what}"), theme::dim()));
    }
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(used + quit.width()).max(2)),
    ));
    spans.push(Span::styled(quit_key, theme::bright()));
    spans.push(Span::styled(quit_what, theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::tui::app::tests::{app, hosts, press, row};
    use std::time::{Duration, Instant};

    use crate::tui::app::{App, Effect, Peek, Spot};

    fn fuller() -> App {
        let mut app = app();
        let mut estimated = row("mini/rig", "working", "working", true);
        estimated.since_estimated = true;
        estimated.pane.tab = "2-toolrig:rig:demo-team#0123456789abcdef#l2".into();
        estimated.pane.title = "修复解析器🙂 wide title".into();
        estimated.pane.msg = None;
        let mut rows = app.rows.clone();
        rows.insert(3, estimated);
        let mut seat = row("local/inferred", "unknown", "unreported", true);
        seat.pane.source = "inferred".into();
        rows.push(seat);
        app.set_rows(rows, app.hosts.clone());
        app
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

    fn screen(width: u16, height: u16) -> String {
        render(&fuller(), width, height)
    }

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
            assert!(text.contains("◉ "), "blocked glyph at {width}");
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
            wide.contains("━━ ◉") && narrow.contains("━━ ◉"),
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
        assert!(!app.hidden);
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
        let text = render(&app, 50, 30);
        assert!(text.contains("▸ NEEDS YOU (2)"), "{text}");
        assert!(text.contains("NEEDS YOU is folded · z opens it"), "{text}");
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
                .contains("serve stopped · r reconnects"),
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
    fn the_narrow_key_bar_keeps_the_essentials_and_question_mark_shows_all() {
        let mut app = fuller();
        let text = render(&app, 50, 30);
        let bar = text.lines().last().unwrap();
        for want in ["j/k move", "s seen", "/ filter", "? keys", "q quit"] {
            assert!(bar.contains(want), "{bar}");
        }
        press(&mut app, KeyCode::Char('?'));
        let text = render(&app, 50, 30);
        for (key, _) in KEYS {
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
        app.jumped(Err(
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
            let lines: Vec<&str> = text.lines().filter(|l| l.starts_with('●')).collect();
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
                assert!(lines[2].trim_start().starts_with("AGE") || lines[2].contains("STATE"));
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
        press(&mut app, KeyCode::Char('A'));
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
        for (key, what) in KEYS {
            assert!(
                what.width() <= 29,
                "{key}: {what:?} is cut in the key table"
            );
        }
        let mut app = fuller();
        press(&mut app, KeyCode::Char('?'));
        let text = render(&app, 120, 30);
        for want in [
            "ctrl+s",
            "v       status, table, harness, host",
            ":       a view by name; tab completes",
            "S A T   sort by state, age, turn",
            "R O W M by harness, host, where, msg",
            "esc     clear filters/sort/peek/folds",
            "f       one status: w d q p e u i",
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
            "f working 1 · done 2 · question 1 · permission 2 · error 1 · unreported 2 · idle 1 · f/esc clears"
        );
        assert_eq!(
            last(&app, 80),
            "f working 1  done 2  question 1  permission 2  error 1  unreported 2  idle 1"
        );
        assert_eq!(last(&app, 50), "f w1 d2 q1 p2 e1 u2 i1 · f/esc clears");
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
            render(&app, 80, 30).contains("no error rows · f f or esc clears"),
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
        press(&mut app, KeyCode::Char('A'));
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
    fn the_footer_ranks_tab_next_above_v_view_and_shows_v_after_filter() {
        let app = fuller();
        let wide = render(&app, 120, 30);
        assert!(
            wide.lines()
                .last()
                .unwrap()
                .contains("/ filter · v view · ? keys"),
            "{wide}"
        );
        for width in [50, 64, 65, 80] {
            let text = render(&app, width, 30);
            let bar = text.lines().last().unwrap();
            assert!(bar.contains("? keys"), "{width}: {bar}");
            assert!(
                !bar.contains("v view") || bar.contains("tab next"),
                "{width}: tab next outranks v view: {bar}"
            );
        }
        let at_80 = render(&app, 80, 30);
        let bar = at_80.lines().last().unwrap();
        assert!(
            bar.starts_with("j/k move · s seen · / filter · ? keys · tab next · enter jump ")
                && bar.trim_end().ends_with(" q quit"),
            "{bar}"
        );
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

    /// The card's rows: from its bar to the line above the footer.
    fn card_of(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let bar = lines.iter().position(|l| l.starts_with("━━")).expect(text);
        lines[bar..lines.len() - 1]
            .iter()
            .map(|l| (*l).to_owned())
            .collect()
    }

    /// The list's row lines: between the column header and the card's bar.
    fn list_rows(text: &str) -> usize {
        let lines: Vec<&str> = text.lines().collect();
        let bar = lines
            .iter()
            .position(|l| l.starts_with("━━"))
            .unwrap_or(lines.len() - 1);
        bar.saturating_sub(3)
    }

    fn asking(msg: &str) -> App {
        let mut app = fuller();
        let mut rows = app.rows.clone();
        rows[0].pane.kind = Some("permission".into());
        rows[0].pane.msg = Some(msg.into());
        app.set_rows(rows, hosts());
        app
    }

    #[test]
    fn the_card_says_who_where_how_and_what_at_80_140_and_200() {
        let app = asking("Bash: git push origin main --force-with-lease");
        for (width, height) in [(80u16, 30u16), (140, 30), (200, 40)] {
            let text = render(&app, width, height);
            let card = card_of(&text);
            let harness = if width == 80 { "" } else { "claude · " };
            assert!(
                card[0].starts_with(&format!(
                    "━━ ◉ blocked · permission · seat-b · api › main · mini · {harness}waiting"
                )),
                "{width}: the bar, its harness giving way at 80:\n{text}"
            );
            assert!(
                card[0].ends_with("━━"),
                "{width}: the bar is full width:\n{text}"
            );
            assert_eq!(
                card[1], " Bash: git push origin main --force-with-lease",
                "{width}"
            );
            assert!(!text.contains('│'), "{width}: no pane on the right");
            let want = (usize::from(height) - 3) * 2 / 5;
            assert!(
                card.len() >= want.max(8).min(usize::from(height) - 6),
                "{width}: {}",
                card.len()
            );
            assert!(list_rows(&text) >= 3, "{width}:\n{text}");
        }
    }

    #[test]
    fn a_done_row_shows_its_headline_then_its_body_at_most_120_wide() {
        let mut app = fuller();
        let long = "word ".repeat(80);
        let mut rows = app.rows.clone();
        let done = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
        done.pane.msg = Some(format!(
            "Opened PR #31; CI is green. ⏎ {long} ⏎ Next step is yours."
        ));
        done.last_turn = Some(372);
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/claude".into()));
        let text = render(&app, 200, 40);
        let card = card_of(&text);
        assert!(
            card[0].contains(" ended ") && card[0].contains(" ago · turn 6m"),
            "{}",
            card[0]
        );
        assert_eq!(card[1], " Opened PR #31; CI is green.");
        assert!(card[2].starts_with("   word word"), "{text}");
        assert!(card[1..].iter().all(|l| l.width() <= 120), "{text}");
        assert!(text.contains("   Next step is yours."), "{text}");
        let list = text
            .lines()
            .find(|l| l.contains(" claude ") && l.contains("Opened"))
            .unwrap();
        assert!(
            list.trim_end().ends_with("CI is green."),
            "MSG is the first line: {list}"
        );
    }

    #[test]
    fn a_blocked_ask_is_never_cut_and_the_list_keeps_3_rows() {
        let ask = (1..=6)
            .map(|n| format!("line {n} of the question"))
            .collect::<Vec<_>>()
            .join(" ⏎ ");
        let app = asking(&ask);
        let text = render(&app, 80, 30);
        for n in 1..=6 {
            assert!(
                text.contains(&format!("line {n} of the question")),
                "{text}"
            );
        }
        assert!(list_rows(&text) >= 3, "{text}");
        let long = (1..=40)
            .map(|n| format!("step {n}"))
            .collect::<Vec<_>>()
            .join(" ⏎ ");
        let text = render(&asking(&long), 80, 30);
        assert_eq!(
            list_rows(&text),
            3,
            "the ask grows to the list's floor:\n{text}"
        );
    }

    /// The board with seat-b's full view open and `lines` read from its pane at 05:42:59.
    fn full_of(lines: Vec<String>) -> App {
        let mut app = fuller();
        press(&mut app, KeyCode::Char(' '));
        app.peeked("mini/seat-b", Ok(lines));
        app.peek.as_mut().unwrap().at = Some(
            DateTime::parse_from_rfc3339("2026-01-01T05:42:59Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        app
    }

    fn numbered(n: usize) -> Vec<String> {
        (1..=n).map(|n| format!("peek line {n}")).collect()
    }

    #[test]
    fn space_opens_the_full_view_and_space_or_esc_returns() {
        let mut app = fuller();
        let effect = press(&mut app, KeyCode::Char(' '));
        assert!(
            matches!(effect, Some(Effect::Peek { ref key, .. }) if key == "mini/seat-b"),
            "{effect:?}"
        );
        let text = render(&app, 80, 30);
        assert!(!text.contains("NEEDS YOU"), "no list:\n{text}");
        assert!(
            text.lines().next().unwrap().ends_with(" capturing… ━━"),
            "{text}"
        );
        assert!(text.contains(" capturing…\n"), "{text}");
        assert_eq!(press(&mut app, KeyCode::Char(' ')), None);
        assert!(app.full.is_none() && render(&app, 80, 30).contains("NEEDS YOU"));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Esc);
        assert!(app.full.is_none(), "esc returns too");
    }

    #[test]
    fn full_view_takes_the_terminal_and_cuts_long_lines_at_80_140_and_200() {
        let long = format!("start {} end", "x".repeat(300));
        let mut lines = numbered(60);
        lines.push(long);
        lines.push("last line".into());
        let app = full_of(lines);
        for width in [80u16, 140, 200] {
            let text = render(&app, width, 30);
            let rows: Vec<&str> = text.lines().collect();
            assert_eq!(rows.len(), 30);
            assert!(
                rows[0].starts_with("━━") && rows[0].contains("seat-b"),
                "{text}"
            );
            assert!(
                rows[0].ends_with(" live · 1s · following ━━"),
                "{}",
                rows[0]
            );
            let footer = match width {
                80 => "space back · j/k scroll · a answer · enter jump · g/G top, bottom",
                _ => {
                    "space back · j/k scroll · a answer · enter jump · g/G top, bottom · pgup/pgdn page · q quit"
                }
            };
            assert_eq!(rows[29], footer, "whole keys only");
            assert_eq!(rows[28], "last line", "the newest line sits on the bottom");
            assert!(
                rows[27].starts_with("start x") && rows[27].ends_with("x…"),
                "cut, not wrapped, and marked: {}",
                rows[27]
            );
            assert_eq!(
                rows[27].width(),
                usize::from(width),
                "full width, no 120 cap"
            );
            assert_eq!(rows[26], "peek line 60");
            assert_eq!(rows[1], "peek line 35", "28 rows of body");
            assert!(!text.contains(" end"), "{text}");
        }
    }

    #[test]
    fn full_view_scrolling_pauses_on_the_lines_it_had_and_g_follows_again() {
        let mut app = full_of(numbered(200));
        app.height = 30;
        press(&mut app, KeyCode::Char('k'));
        let text = render(&app, 80, 30);
        assert!(
            text.lines().next().unwrap().contains("paused · G follows"),
            "{text}"
        );
        assert_eq!(text.lines().nth(28), Some("peek line 199"));
        app.peeked("mini/seat-b", Ok(numbered(201)));
        let text = render(&app, 80, 30);
        assert!(
            text.lines()
                .next()
                .unwrap()
                .contains(" paused · newer below · G follows ━━"),
            "{text}"
        );
        assert!(!text.contains("peek line 201"), "held while paused");
        press(&mut app, KeyCode::PageUp);
        assert_eq!(render(&app, 80, 30).lines().nth(28), Some("peek line 171"));
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(render(&app, 80, 30).lines().nth(1), Some("peek line 1"));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(render(&app, 80, 30).lines().nth(1), Some("peek line 2"));
        press(&mut app, KeyCode::Char('G'));
        let text = render(&app, 80, 30);
        assert_eq!(
            text.lines().nth(28),
            Some("peek line 201"),
            "follows the newest read"
        );
        assert!(text.lines().next().unwrap().contains("following"), "{text}");
    }

    #[test]
    fn full_view_keeps_its_lines_when_a_read_fails() {
        let mut app = full_of(numbered(40));
        app.peeked("mini/seat-b", Err("rex timed out".into()));
        let text = render(&app, 80, 30);
        assert!(
            text.lines()
                .next()
                .unwrap()
                .ends_with(" rex timed out · 1s old · retrying ━━"),
            "{text}"
        );
        assert_eq!(text.lines().nth(28), Some("peek line 40"));
        app.peeked("mini/seat-b", Ok(numbered(41)));
        let text = render(&app, 80, 30);
        assert!(
            !text.contains("retrying") && text.contains("peek line 41"),
            "{text}"
        );
    }

    #[test]
    fn full_view_reads_its_row_every_two_seconds_and_stops_on_close() {
        use crate::tui::full::EVERY;
        let mut app = fuller();
        press(&mut app, KeyCode::Char(' '));
        let t0 = app.full.as_ref().unwrap().asked;
        assert_eq!(app.live(t0 + EVERY * 5), None, "never two reads at once");
        app.peeked("mini/seat-b", Ok(numbered(3)));
        assert_eq!(app.live(t0 + Duration::from_secs(1)), None);
        let effect = app.live(t0 + EVERY);
        assert!(
            matches!(effect, Some(Effect::Peek { ref key, .. }) if key == "mini/seat-b"),
            "{effect:?}"
        );
        assert_eq!(app.live(t0 + EVERY * 2), None, "the read is out");
        assert_eq!(
            app.auto_tail(t0 + EVERY * 30),
            None,
            "the view does its own reads"
        );
        app.peeked("mini/seat-b", Ok(numbered(4)));
        assert!(app.live(t0 + EVERY * 2).is_some());
        press(&mut app, KeyCode::Esc);
        app.peeked("mini/seat-b", Ok(numbered(5)));
        assert_eq!(app.live(t0 + EVERY * 60), None, "closed, it reads no more");
    }

    #[test]
    fn full_view_keys_answer_jump_quit_and_leave_the_board_keys_alone() {
        let mut app = full_of(numbered(3));
        assert_eq!(
            press(&mut app, KeyCode::Char('s')),
            None,
            "s does not mark seen"
        );
        assert!(app.full.is_some());
        assert!(
            matches!(press(&mut app, KeyCode::Enter), Some(Effect::Jump(row)) if row.key == "mini/seat-b")
        );
        assert_eq!(press(&mut app, KeyCode::Char('q')), Some(Effect::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle(ctrl_c), Some(Effect::Quit));
        press(&mut app, KeyCode::Char('a'));
        assert!(app.full.is_none());
        assert_eq!(
            app.answer.as_ref().map(|answer| answer.row.key.as_str()),
            Some("mini/seat-b")
        );
    }

    #[test]
    fn full_view_says_why_a_first_read_failed_and_shows_a_failed_jump() {
        let mut app = fuller();
        press(&mut app, KeyCode::Char(' '));
        app.peeked("mini/seat-b", Err("rex timed out".into()));
        let text = render(&app, 80, 30);
        assert!(
            text.lines()
                .next()
                .unwrap()
                .ends_with(" rex timed out · retrying ━━"),
            "{text}"
        );
        assert_eq!(text.lines().nth(1), Some(" rex timed out"));
        assert!(
            app.live(app.full.as_ref().unwrap().asked + crate::tui::full::EVERY)
                .is_some()
        );
        app.failed = Some("that pane is gone".into());
        assert_eq!(
            render(&app, 80, 30).lines().last(),
            Some("that pane is gone")
        );
    }

    #[test]
    fn full_view_cuts_a_long_failure_so_the_name_keeps_its_room() {
        let mut app = full_of(numbered(3));
        app.peeked(
            "mini/seat-b",
            Err(format!("peek failed: {}", "detail ".repeat(20))),
        );
        let bar = render(&app, 80, 30).lines().next().unwrap().to_owned();
        assert!(bar.contains("seat-b"), "{bar}");
        assert!(bar.ends_with("… · 1s old · retrying ━━"), "{bar}");
    }

    #[test]
    fn full_view_closes_when_its_row_goes() {
        let mut app = full_of(numbered(3));
        let rows = app
            .rows
            .iter()
            .filter(|row| row.key != "mini/seat-b")
            .cloned()
            .collect();
        app.set_rows(rows, hosts());
        assert!(app.full.is_none() && render(&app, 80, 30).contains("WORKING"));
    }

    #[test]
    fn c_hides_the_card_and_keeps_the_choice() {
        let mut app = fuller();
        let effect = press(&mut app, KeyCode::Char('c'));
        assert!(matches!(effect, Some(Effect::SavePrefs(prefs)) if prefs.detail_hidden));
        assert_eq!(app.toast(), Some("the card is hidden · c shows it"));
        app.toast = None;
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
        assert!(render(&app, 140, 30).contains("━━ ◉"));
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
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.current().unwrap().key, "local/claude");
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('c'));
        assert!(!app.hidden, "c after f belongs to f");
        assert_eq!(app.toast(), Some("f takes w d q p e u i; f or esc clears"));
        press(&mut app, KeyCode::Char('S'));
        assert!(app.sort.is_some(), "S still sorts");
    }

    #[test]
    fn the_bar_says_how_fresh_the_tail_is() {
        let mut app = fuller();
        let at = DateTime::parse_from_rfc3339("2026-01-01T05:42:52Z")
            .unwrap()
            .with_timezone(&Utc);
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(vec!["tail line".into()])),
            at: Some(at),
            updated_at: Some("2026-01-01T00:00:00Z".into()),
        });
        let time = at.with_timezone(&Local).format("%H:%M:%S").to_string();
        let text = render(&app, 140, 30);
        assert!(
            card_of(&text)[0].ends_with(&format!(" captured {time} · 8s ━━")),
            "{text}"
        );
        assert!(
            text.contains(" │ tail line"),
            "the tail sits behind a gutter:\n{text}"
        );
        app.peek.as_mut().unwrap().updated_at = Some("2025-12-31T23:00:00Z".into());
        let text = render(&app, 140, 30);
        assert!(
            card_of(&text)[0].ends_with(" newer report · space refreshes ━━"),
            "{text}"
        );
    }

    #[test]
    fn a_long_window_gives_way_in_the_bar_so_the_outcome_shows() {
        let mut app = asking("Bash: ls");
        let mut rows = app.rows.clone();
        rows[0].pane.tab = "2-toolrig:rig:demo-team#fedcba9876543210#l2".into();
        app.set_rows(rows, hosts());
        let text = render(&app, 80, 30);
        let bar = &card_of(&text)[0];
        assert!(bar.contains("…") && bar.contains(" · waiting "), "{bar}");
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(vec!["tail line".into()])),
            at: Some(Utc::now()),
            updated_at: None,
        });
        let text = render(&app, 80, 30);
        let bar = &card_of(&text)[0];
        assert!(
            bar.starts_with("━━ ◉ blocked · permission · seat-b · waiting ")
                && bar.contains("captured"),
            "the place gives way too: {bar}"
        );
    }

    #[test]
    fn a_row_with_no_record_says_so_and_how_to_look() {
        let mut app = fuller();
        let mut rows = app.rows.clone();
        rows.iter_mut()
            .find(|r| r.key == "local/inferred")
            .unwrap()
            .pane
            .msg = None;
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/inferred".into()));
        let card = card_of(&render(&app, 80, 30));
        assert_eq!(card[1], " no status from this pane");
        assert_eq!(card[2], "   space captures the screen");
    }

    #[test]
    fn angle_brackets_say_once_that_they_are_gone() {
        let mut app = fuller();
        assert_eq!(press(&mut app, KeyCode::Char('>')), None);
        assert_eq!(
            app.toast(),
            Some("the card spans the board now, so < and > are gone · c hides it")
        );
        app.toast = None;
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.toast(), None, "once a run");
    }

    const PERMISSION: &str = "⏺ Bash(git push origin main --force-with-lease)\n Bash command\n   git push origin main --force-with-lease\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and don't ask again for git push commands in /work/project\n   3. No, and tell Claude what to do differently\n\n Esc to cancel · Tab to amend\n";

    fn answering(text: &str) -> App {
        let mut app = fuller();
        let Some(Effect::Read { key, .. }) = press(&mut app, KeyCode::Char('a')) else {
            panic!("a reads the pane");
        };
        app.screen(&key, Ok(crate::tui::screen::Screen::parse(text, None)));
        app
    }

    #[test]
    fn answer_mode_shows_a_red_bar_the_options_and_its_own_footer_at_80_and_140() {
        let app = answering(PERMISSION);
        for width in [80, 140] {
            let text = render(&app, width, 24);
            let card = card_of(&text);
            assert!(
                card[0].starts_with("━━ ANSWER → seat-b · api › main · mini · claude "),
                "{text}"
            );
            assert!(card[0].contains("captured"), "{text}");
            assert!(card.iter().any(|l| l == " │  ❯ 1. Yes"), "{text}");
            assert!(
                card.iter()
                    .any(|l| l.contains("2. Yes, and don't ask") && l.ends_with("press 2 twice")),
                "{text}"
            );
            assert!(
                !text.contains("Esc to cancel"),
                "chrome below the options is cut: {text}"
            );
            assert_eq!(
                card.last().unwrap(),
                " ›",
                "no cursor under a dialog: {text}"
            );
            let footer = text.lines().last().unwrap();
            assert!(
                footer.starts_with("1-3 pick · esc back to board"),
                "{footer}"
            );
            assert_eq!(
                footer.contains("board keys are off"),
                width == 140,
                "the promise shows when it fits: {footer}"
            );
        }
    }

    #[test]
    fn answer_mode_says_why_a_key_did_nothing_and_shows_a_pending_double_press() {
        let mut app = answering(PERMISSION);
        press(&mut app, KeyCode::Char('x'));
        let text = render(&app, 100, 24);
        assert!(
            text.contains(" pick a number; this dialog takes no text"),
            "{text}"
        );
        press(&mut app, KeyCode::Char('2'));
        let text = render(&app, 100, 24);
        assert!(
            text.contains(" press 2 again to send it: Yes, and don't ask again"),
            "{text}"
        );
    }

    #[test]
    fn after_a_send_the_card_drops_the_tail_from_before_the_answer() {
        let mut app = answering(PERMISSION);
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(PERMISSION.lines().map(str::to_owned).collect())),
            at: Some(Utc::now()),
            updated_at: None,
        });
        press(&mut app, KeyCode::Char('1'));
        app.sent("mini/seat-b", "1 (Yes)", Ok(()));
        assert!(app.peek.is_none());
        let card = card_of(&render(&app, 140, 24)).join("\n");
        assert!(!card.contains("Do you want to proceed"), "{card}");
        assert!(card.contains(" sent 1 (Yes) → seat-b "), "{card}");
    }

    #[test]
    fn after_a_send_the_bar_says_what_went_where() {
        let mut app = answering(PERMISSION);
        press(&mut app, KeyCode::Char('1'));
        app.sent("mini/seat-b", "1 (Yes)", Ok(()));
        let text = render(&app, 140, 24);
        let bar = &card_of(&text)[0];
        assert!(bar.contains(" sent 1 (Yes) → seat-b "), "{bar}");
    }

    #[test]
    fn answer_a_short_board_shows_every_option_or_refuses_the_hidden_ones() {
        let four = include_str!("../../tests/fixtures/screens/claude-permission.txt");
        for text in [PERMISSION, four] {
            for (height, refused) in [(12, false), (9, false), (13, true), (6, false)] {
                let mut app = answering(text);
                if refused {
                    app.answer.as_mut().unwrap().error =
                        Some("the screen changed since it was read; nothing sent".into());
                }
                let screen = render(&app, 80, height);
                let footer = screen.lines().last().unwrap().to_owned();
                let digits: Vec<char> = app
                    .answer
                    .as_ref()
                    .unwrap()
                    .screen
                    .as_ref()
                    .unwrap()
                    .options
                    .iter()
                    .map(|c| c.digit)
                    .collect();
                let picks = footer
                    .split(" pick")
                    .next()
                    .filter(|_| footer.contains(" pick"));
                let shown: Vec<char> = match picks.map(|p| p.split_once('-')) {
                    Some(Some((a, b))) => {
                        let (a, b) = (a.chars().next().unwrap(), b.chars().next().unwrap());
                        digits
                            .iter()
                            .copied()
                            .filter(|d| (a..=b).contains(d))
                            .collect()
                    }
                    Some(None) => picks.unwrap().chars().take(1).collect(),
                    None => Vec::new(),
                };
                if height >= 12 {
                    assert_eq!(shown, digits, "every option fits at 80x{height}: {screen}");
                }
                for digit in digits {
                    let drawn = screen.lines().any(|l| l.contains(&format!("{digit}. ")));
                    let mut copy = answering(text);
                    render(&copy, 80, height);
                    let sent = press(&mut copy, KeyCode::Char(digit));
                    if shown.contains(&digit) {
                        assert!(drawn, "{digit} is in the footer, so on the card: {screen}");
                    } else {
                        assert_eq!(sent, None, "{digit} is off screen at 80x{height}");
                        let why = copy.answer.as_ref().unwrap().error.clone().unwrap();
                        assert_eq!(
                            why,
                            format!(
                                "option {digit} is off screen; make the board taller or jump to it"
                            )
                        );
                    }
                }
            }
        }
    }
}
