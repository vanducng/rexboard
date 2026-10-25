use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Item, Link, Spot};
use super::theme;
use crate::board::Row;
use crate::output::{clean, fit, harness, in_state, name, state_word, what, wheres};
use crate::pane::bare_title;

/// Width of the detail pane when it sits on the right.
const DETAIL: u16 = 38;

const STOPPED: &str = "serve stopped · r reconnects";

/// The `?` table: every key, in the order an operator reaches for them.
pub const KEYS: [(&str, &str); 12] = [
    ("j/k", "move; g/G first, last"),
    ("1-9", "the nth NEEDS YOU row"),
    ("s", "mark seen"),
    ("S", "mark shown done rows seen"),
    ("space", "peek at the pane"),
    ("/", "filter; enter keeps it"),
    ("esc", "clear filter, peek, folds"),
    ("z", "fold or open a group"),
    ("enter", "go to the pane"),
    ("r", "reconnect to serve"),
    ("?", "this table"),
    ("q", "quit"),
];

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state word and the tab, with the detail pane on the right.
    Wide,
}

fn tier(width: u16) -> Tier {
    match width {
        0..60 => Tier::Narrow,
        60..100 => Tier::Mid,
        _ => Tier::Wide,
    }
}

pub fn draw(frame: &mut Frame, app: &App, now: DateTime<Utc>) {
    let area = frame.area();
    let tier = tier(area.width);
    let ruled = tier != Tier::Wide && area.height >= 20;
    let [top, body, above_footer, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(u16::from(ruled)),
        Constraint::Length(1),
    ])
    .areas(area);
    header(frame, top, app, now);
    if tier == Tier::Wide && !app.pick {
        let [list_area, rule, detail_area] = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(1),
            Constraint::Length(DETAIL),
        ])
        .areas(body);
        list(frame, list_area, app, tier, now);
        let bar = vec![Line::styled("│", theme::dim()); usize::from(rule.height)];
        frame.render_widget(Paragraph::new(bar), rule);
        let padded = Rect {
            x: detail_area.x + 1,
            width: detail_area.width.saturating_sub(1),
            ..detail_area
        };
        let lines = detail(app, now, usize::from(padded.width), true);
        frame.render_widget(Paragraph::new(lines), padded);
    } else {
        let lines = if app.pick {
            Vec::new()
        } else {
            detail(app, now, usize::from(body.width), false)
        };
        let height = (lines.len() as u16).min(body.height / 2);
        let [list_area, rule, detail_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(u16::from(height > 0)),
            Constraint::Length(height),
        ])
        .areas(body);
        list(frame, list_area, app, tier, now);
        frame.render_widget(rule_line(rule.width), rule);
        // A peek keeps its newest lines when the pane is short.
        let skip = lines.len().saturating_sub(usize::from(height));
        let lines = if skip > 0 && app.peek.is_some() {
            std::iter::once(lines[0].clone())
                .chain(lines.into_iter().skip(skip + 1))
                .collect()
        } else {
            lines
        };
        frame.render_widget(Paragraph::new(lines), detail_area);
    }
    frame.render_widget(rule_line(above_footer.width), above_footer);
    footer(frame, bottom, app);
}

fn rule_line(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(theme::dim())
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
        " · {} hosts · {} panes · {} agents",
        app.hosts.len(),
        app.panes,
        app.rows.len()
    );
    let healthy = app.hosts.keys().cloned().collect::<Vec<_>>().join(" + ");
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
            left + right < width
        })
        .unwrap_or((false, false, false));
    let mut left = vec![Span::styled("rexboard", theme::accent())];
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

fn list(frame: &mut Frame, area: Rect, app: &App, tier: Tier, now: DateTime<Utc>) {
    let items = app.items();
    if items.is_empty() {
        let query = app.filter.value();
        let text = if !query.is_empty() {
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
    let height = usize::from(area.height);
    let room = if items.len() > height {
        height.saturating_sub(1)
    } else {
        height
    };
    let at = items.iter().position(|item| is_selected(app, item));
    let offset = at.map_or(0, |at| (at + 1).saturating_sub(room));
    let width = usize::from(area.width);
    let places = wheres(
        &app.rows,
        where_what(tier, width).0.saturating_sub(1),
        tier != Tier::Wide,
        tier >= Tier::Mid,
    );
    let mut lines: Vec<Line> = items
        .iter()
        .skip(offset)
        .take(room)
        .map(|item| match *item {
            Item::Header {
                group,
                count,
                folded,
            } => {
                let mark = if folded { "▸ " } else { "" };
                let title = format!("{mark}{} ({count}) ", theme::group_title(group));
                let fill = "┄".repeat(width.saturating_sub(title.width()));
                let line = Line::styled(format!("{title}{fill}"), theme::header());
                if is_selected(app, item) {
                    line.style(theme::header().bg(theme::RAISED))
                } else {
                    line
                }
            }
            Item::Row(i) => row_line(
                &app.rows[i],
                &places[i],
                app,
                tier,
                width,
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
                group,
                folded: true,
                ..
            },
            Some(Spot::Group(g)),
        ) => group == g,
        _ => false,
    }
}

/// The columns before WHERE: the glyph, the state word (wide), time in state, harness and host.
fn lead_width(tier: Tier) -> usize {
    match tier {
        Tier::Narrow => 9,
        Tier::Mid => 25,
        Tier::Wide => 34,
    }
}

/// How wide WHERE and WHAT are in a row `width` columns wide.
fn where_what(tier: Tier, width: usize) -> (usize, usize) {
    let rest = width.saturating_sub(lead_width(tier));
    let share = if tier == Tier::Wide {
        rest * 5 / 9
    } else {
        rest * 9 / 20
    };
    (share, rest - share)
}

/// One row of the list. `place` is its WHERE, worked out with the other rows.
fn row_line(
    row: &Row,
    place: &str,
    app: &App,
    tier: Tier,
    width: usize,
    selected: bool,
    now: DateTime<Utc>,
) -> Line<'static> {
    let faded = row.stale || app.link != Link::Up;
    let soft = if row.pane.source == "inferred" || row.suspect || faded {
        theme::dim()
    } else {
        Style::new()
    };
    let mut cells: Vec<(String, Style)> =
        vec![(format!("{} ", theme::glyph(row)), theme::state(row))];
    if tier == Tier::Wide {
        cells.push((fit(&state_word(row), 8) + " ", theme::state(row)));
    }
    cells.push((fit(&in_state(row, now), 6) + " ", soft));
    if tier >= Tier::Mid {
        cells.push((fit(&harness(row), 7) + " ", soft));
        cells.push((fit(&row.pane.host, 7) + " ", theme::dim()));
    }
    debug_assert_eq!(
        cells.iter().map(|(text, _)| text.width()).sum::<usize>(),
        lead_width(tier)
    );
    let (share, what_width) = where_what(tier, width);
    let room = share.saturating_sub(1);
    cells.push((fit(place, room) + " ", soft));
    cells.push((fit(what(row), what_width), soft));
    let mut spans: Vec<Span> = cells
        .into_iter()
        .map(|(text, style)| Span::styled(text, style))
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

/// The detail pane's lines: the key table, a folded group's hint, or the selected row. Below
/// 100 columns it leaves out what the row already shows, and a peek takes the fields' place.
fn detail(app: &App, now: DateTime<Utc>, width: usize, wide: bool) -> Vec<Line<'static>> {
    if app.keys {
        let mut lines = vec![Line::styled(fit("keys", width), theme::accent())];
        lines.extend(KEYS.iter().map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:<6} "), theme::bright()),
                Span::styled(fit(what, width.saturating_sub(7)), theme::dim()),
            ])
        }));
        return lines;
    }
    let Some(row) = app.current() else {
        return match &app.selected {
            Some(Spot::Group(group)) => vec![Line::styled(
                fit(
                    &format!("{} is folded · z opens it", theme::group_title(*group)),
                    width,
                ),
                theme::dim(),
            )],
            _ => Vec::new(),
        };
    };
    let p = &row.pane;
    let mut lines = vec![Line::styled(fit(&name(row), width), theme::accent())];
    if let Some(why) = &app.failed {
        // A failed jump is worth reading whole, as a peek error is.
        lines.extend(
            wrap(why, width.max(1))
                .into_iter()
                .map(|line| Line::styled(line, Style::new().fg(Color::Red))),
        );
    }
    let peek = app.peek.as_ref().filter(|peek| peek.key == row.key);
    if let (false, Some(peek)) = (wide, peek) {
        lines.extend(peek_lines(peek, width));
        return lines;
    }
    let mut state = match &p.kind {
        Some(kind) => format!("{} · {kind}", p.state),
        None => p.state.clone(),
    };
    if row.suspect {
        state.push_str(" (reported)");
    }
    let reported = p
        .updated_at
        .as_deref()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map_or("none".to_owned(), |at| {
            at.with_timezone(&Local).format("%H:%M:%S").to_string()
        });
    let dash = |text: &str| {
        if text.is_empty() {
            "-".to_owned()
        } else {
            text.to_owned()
        }
    };
    if let Some(why) = &row.suspect_reason {
        lines.extend(
            wrap(&format!("may be stale: {why}"), width.max(1))
                .into_iter()
                .map(|line| Line::styled(line, theme::dim())),
        );
    }
    let value = width.saturating_sub(10);
    let pair = |key: &str, val: &str| {
        Line::from(vec![
            Span::styled(format!("{key:<9} "), theme::dim()),
            Span::raw(fit(val, value)),
        ])
    };
    lines.push(pair("state", &state));
    let msg = clean(p.msg.as_deref().unwrap_or_default());
    if wide {
        lines.push(pair("in state", &in_state(row, now)));
    }
    // Wrapped: a permission prompt is what you read before acting. Underneath, only when the
    // row cut it.
    let show = if wide {
        Some(3)
    } else {
        (msg.width() > where_what(tier(width as u16), width).1).then_some(2)
    };
    if let Some(most) = show {
        let mut wrapped = wrap(&dash(&msg), value.max(1));
        if wrapped.len() > most {
            wrapped.truncate(most);
            wrapped[most - 1] = fit(&format!("{}…", wrapped[most - 1]), value)
                .trim_end()
                .to_owned();
        }
        for (i, line) in wrapped.iter().enumerate() {
            lines.push(pair(if i == 0 { "msg" } else { "" }, line));
        }
    }
    let mut pairs = Vec::new();
    pairs.extend([
        ("host", p.host.clone()),
        ("session", p.session.clone()),
        ("tab", p.tab.clone()),
        ("title", dash(bare_title(&p.title))),
        ("harness", p.harness.clone().unwrap_or_else(|| "-".into())),
        ("source", p.source.clone()),
        ("last rpt", reported),
    ]);
    for (key, val) in pairs {
        lines.push(pair(key, &val));
    }
    if p.source == "inferred" {
        lines.push(Line::styled(
            fit("no status record; state from the title", width),
            theme::dim(),
        ));
    }
    lines.push(Line::styled(
        fit("enter goes to the pane", width),
        theme::dim(),
    ));
    if let Some(peek) = peek {
        lines.push(Line::styled("peek", theme::header()));
        lines.extend(peek_lines(peek, width));
    }
    lines
}

fn peek_lines(peek: &super::app::Peek, width: usize) -> Vec<Line<'static>> {
    match &peek.lines {
        None => vec![Line::styled("capturing…", theme::dim())],
        Some(Ok(text)) => text
            .iter()
            .map(|line| Line::raw(fit(line, width)))
            .collect(),
        // An error is worth reading whole, so it wraps instead of being cut.
        Some(Err(why)) => wrap(why, width.max(1))
            .into_iter()
            .map(|line| Line::styled(line, Style::new().fg(Color::Red)))
            .collect(),
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
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

fn footer(frame: &mut Frame, area: Rect, app: &App) {
    let width = usize::from(area.width);
    if app.filtering {
        let text = format!("/ {}▏", app.filter.value());
        frame.render_widget(
            Paragraph::new(Line::styled(fit(&text, width), theme::bright())),
            area,
        );
        return;
    }
    if let Some(toast) = app.toast() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                fit(toast, width),
                Style::new().fg(theme::ACCENT),
            )),
            area,
        );
        return;
    }
    let held = app
        .going
        .as_deref()
        .map(|text| (text, Style::new().fg(theme::ACCENT)))
        .or_else(|| {
            app.failed
                .as_deref()
                .map(|text| (text, Style::new().fg(Color::Red)))
        });
    if let Some((text, style)) = held {
        frame.render_widget(Paragraph::new(Line::styled(fit(text, width), style)), area);
        return;
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    if app.peek.is_some() {
        used += "space closes · ".width();
        spans.push(Span::styled("space closes · ", theme::dim()));
    }
    let query = app.filter.value();
    if !query.is_empty() {
        let kept = fit(&format!("/{query}"), 16).trim_end().to_owned();
        used += kept.width() + " · esc clears · ".width();
        spans.push(Span::styled(kept, theme::bright()));
        spans.push(Span::styled(" · esc clears · ", theme::dim()));
    }
    // The essentials first, so a narrow board keeps them.
    let keys: &[(&str, &str)] = if app.pick {
        &[("enter", "go"), ("j/k", "move"), ("/", "filter")]
    } else {
        &[
            ("j/k", "move"),
            ("s", "seen"),
            ("/", "filter"),
            ("?", "keys"),
            ("space", "peek"),
            ("enter", "jump"),
            ("1-9", "select"),
        ]
    };
    let (quit_key, quit_what) = if app.pick {
        ("esc", " cancel")
    } else {
        ("q", " quit")
    };
    let quit = format!("{quit_key}{quit_what}");
    let mut first = true;
    for &(key, what) in keys {
        if key == "space" && app.peek.is_some() {
            continue;
        }
        let sep = if first { "" } else { " · " };
        let piece = sep.width() + key.width() + 1 + what.width();
        if used + piece + quit.width() + 2 > width {
            break;
        }
        spans.push(Span::styled(sep, theme::dim()));
        spans.push(Span::styled(key, theme::bright()));
        spans.push(Span::styled(format!(" {what}"), theme::dim()));
        used += piece;
        first = false;
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
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::tui::app::tests::{app, hosts, press, row};
    use crate::tui::app::{App, Effect, Peek};

    fn fuller() -> App {
        let mut app = app();
        let mut estimated = row("mini/rig", "working", "working", true);
        estimated.since_estimated = true;
        estimated.pane.tab = "2-openrig:rig:demo-team#0123456789abcdef#l2".into();
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
        assert!(seat_line(&wide).contains("api › main › seat-b"), "{wide}");
        assert!(wide.contains("│"), "the detail pane is on the right at 140");
        assert!(narrow.contains("─────"), "and underneath below 100");
        assert!(
            wide.contains("2-openrig:ri…89abcdef#l2 › rig"),
            "the pane stays whole:\n{wide}"
        );
    }

    #[test]
    fn a_row_with_no_record_shows_a_dash_for_its_age() {
        let wide = screen(140, 40);
        let line = wide.lines().find(|l| l.contains("inferred msg")).unwrap();
        assert!(line.contains(" -  "), "{line}");
    }

    #[test]
    fn a_peek_shows_in_the_narrow_layout() {
        let mut app = fuller();
        let lines = (1..=12).map(|n| format!("peek line {n}")).collect();
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(lines)),
        });
        for (width, height) in [(50, 30), (80, 30)] {
            let text = render(&app, width, height);
            assert!(text.contains("peek line 12"), "{width}x{height}:\n{text}");
            assert!(
                !text.contains("harness"),
                "the peek takes the fields' place:\n{text}"
            );
        }
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
                text.lines().any(|l| l.starts_with(key)),
                "{key} missing:\n{text}"
            );
        }
    }

    #[test]
    fn a_narrow_detail_keeps_a_message_the_row_cuts() {
        let mut app = app();
        let mut rows = app.rows.clone();
        rows[0].pane.msg = Some(
            "Allow Bash: cargo test --workspace --all-features -- --test-threads 1 \
             and then a lot more words that will not fit in two lines at all"
                .into(),
        );
        app.set_rows(rows, hosts());
        let text = render(&app, 50, 30);
        let at = text
            .lines()
            .position(|l| l.starts_with("msg "))
            .expect(&text);
        let next = text.lines().nth(at + 1).unwrap();
        assert!(
            next.starts_with("          ") && next.ends_with('…'),
            "{text}"
        );
        assert!(
            text.lines().nth(at + 2).unwrap().starts_with("host"),
            "{text}"
        );
        let text = render(&self::app(), 50, 30);
        assert!(
            !text.lines().any(|l| l.starts_with("msg ")),
            "a message the row shows whole stays out:\n{text}"
        );
    }

    #[test]
    fn the_wide_detail_wraps_a_message_to_three_lines() {
        let mut app = app();
        let mut rows = app.rows.clone();
        rows[0].pane.msg = Some(
            "Allow Bash: cargo test --workspace --all-features -- --test-threads 1 \
             and then a lot more words that will never fit in three lines of the detail"
                .into(),
        );
        app.set_rows(rows, hosts());
        let text = render(&app, 140, 40);
        let detail: Vec<&str> = text
            .lines()
            .map(|l| l.split('│').nth(1).unwrap_or("").trim_start())
            .collect();
        let at = detail
            .iter()
            .position(|l| l.starts_with("msg "))
            .expect(&text);
        assert!(detail[at].contains("Allow Bash:"), "{text}");
        assert!(detail[at + 2].ends_with('…'), "{text}");
        assert!(detail[at + 3].starts_with("host"), "{text}");
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
        assert!(text.contains("state     working (reported)"), "{text}");
    }

    #[test]
    fn an_open_peek_says_how_to_close_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char(' '));
        let text = render(&app, 80, 30);
        let bar = text.lines().last().unwrap();
        assert!(bar.starts_with("space closes · "), "{text}");
        assert!(
            !bar.contains("space peek"),
            "one space key at a time: {bar}"
        );
    }

    #[test]
    fn pick_shows_the_list_alone_with_its_own_keys() {
        let mut app = app();
        app.pick = true;
        for width in [50, 140] {
            let text = render(&app, width, 30);
            assert!(
                !text.contains("state     blocked"),
                "no detail pane:\n{text}"
            );
            let bar = text.lines().last().unwrap();
            assert!(bar.starts_with("enter go · j/k move"), "{bar}");
            assert!(bar.ends_with("esc cancel"), "{bar}");
        }
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Quit));
    }

    #[test]
    fn at_120_columns_where_keeps_what_tells_rows_apart() {
        let mut app = App::default();
        let tab = "2-openrig:rig:demo-team#0123456789abcdef#l2";
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
        let text = render(&app, 120, 36);
        for want in [
            "api-gateway › … › seat-r",
            "web-frontend › … › seat-r",
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
    fn rows_below_the_list_are_counted() {
        let text = render(&fuller(), 80, 14);
        assert!(text.contains("↓ "), "{text}");
        assert!(text.contains(" more"), "{text}");
    }
}
