use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Item, Spot};
use super::theme;
use crate::board::Row;
use crate::output::{fit, harness, in_state, path, what};
use crate::pane::bare_title;

/// Width of the detail pane when it sits on the right.
const DETAIL: u16 = 38;

const STOPPED: &str = "serve stopped · r reconnects";

/// The `?` table: every key, in the order an operator reaches for them.
pub const KEYS: [(&str, &str); 11] = [
    ("j/k", "move; g/G first, last"),
    ("1-9", "the nth NEEDS YOU row"),
    ("s", "mark seen"),
    ("space", "peek at the pane"),
    ("/", "filter; enter keeps it"),
    ("esc", "clear filter, peek, folds"),
    ("z", "fold or open a group"),
    ("enter", "jump (later version)"),
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
    if tier == Tier::Wide {
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
        let lines = detail(app, now, usize::from(body.width), false);
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
    let alert = if app.connected {
        app.hosts
            .iter()
            .filter(|(_, state)| *state != "up")
            .map(|(host, state)| format!("{host} {state}"))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        STOPPED.to_owned()
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
        left.push(Span::styled(alert, Style::new().fg(Color::Red)));
    }
    let mut right = Vec::new();
    if with_clock {
        right.push(Span::styled(clock, theme::dim()));
    }
    if with_hosts && app.connected && app.hosts.values().all(|s| s == "up") {
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
        } else if app.connected {
            "no agent panes yet".to_owned()
        } else {
            "waiting for serve".to_owned()
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
            Item::Row(i) => row_line(&app.rows[i], app, tier, width, is_selected(app, item), now),
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

fn row_line(
    row: &Row,
    app: &App,
    tier: Tier,
    width: usize,
    selected: bool,
    now: DateTime<Utc>,
) -> Line<'static> {
    let faded = row.stale || !app.connected;
    let soft = if row.pane.source == "inferred" || faded {
        theme::dim()
    } else {
        Style::new()
    };
    let mut cells: Vec<(String, Style)> =
        vec![(format!("{} ", theme::glyph(row)), theme::state(row))];
    if tier == Tier::Wide {
        cells.push((fit(&row.pane.state, 8) + " ", theme::state(row)));
    }
    cells.push((fit(&in_state(row, now), 6) + " ", soft));
    if tier >= Tier::Mid {
        cells.push((fit(&harness(row), 7) + " ", soft));
        cells.push((fit(&row.pane.host, 7) + " ", theme::dim()));
    }
    let fixed: usize = cells.iter().map(|(text, _)| text.width()).sum();
    let rest = width.saturating_sub(fixed);
    let p = &row.pane;
    let (labels, share): (Vec<&str>, usize) = if tier == Tier::Wide {
        (vec![&p.session, &p.tab, &p.pane], rest * 5 / 9)
    } else {
        (vec![&p.session, &p.pane], rest * 9 / 20)
    };
    let room = share.saturating_sub(1);
    cells.push((fit(&path(&labels, room), room) + " ", soft));
    cells.push((fit(what(row), rest.saturating_sub(share)), soft));
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
    let mut lines = vec![Line::styled(fit(&p.pane, width), theme::accent())];
    let peek = app.peek.as_ref().filter(|peek| peek.key == row.key);
    if let (false, Some(peek)) = (wide, peek) {
        lines.extend(peek_lines(peek, width));
        return lines;
    }
    let state = match &p.kind {
        Some(kind) => format!("{} · {kind}", p.state),
        None => p.state.clone(),
    };
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
    let mut pairs = vec![("state", state)];
    if wide {
        pairs.push(("in state", in_state(row, now)));
        pairs.push(("msg", dash(p.msg.as_deref().unwrap_or_default())));
    }
    pairs.extend([
        ("host", p.host.clone()),
        ("session", p.session.clone()),
        ("tab", p.tab.clone()),
        ("title", dash(bare_title(&p.title))),
        ("harness", p.harness.clone().unwrap_or_else(|| "-".into())),
        ("source", p.source.clone()),
        ("last rpt", reported),
    ]);
    let value = width.saturating_sub(10);
    for (key, val) in pairs {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:<9} "), theme::dim()),
            Span::raw(fit(&val, value)),
        ]));
    }
    if p.source == "inferred" {
        lines.push(Line::styled(
            fit("no status record; state from the title", width),
            theme::dim(),
        ));
    }
    lines.push(Line::styled(
        fit("enter jumps in a later version", width),
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
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    let query = app.filter.value();
    if !query.is_empty() {
        let kept = fit(&format!("/{query}"), 16).trim_end().to_owned();
        used += kept.width() + " · esc clears · ".width();
        spans.push(Span::styled(kept, theme::bright()));
        spans.push(Span::styled(" · esc clears · ", theme::dim()));
    }
    // The essentials first, so a narrow board keeps them.
    let keys = [
        ("j/k", "move"),
        ("s", "seen"),
        ("/", "filter"),
        ("?", "keys"),
        ("space", "peek"),
        ("enter", "jump"),
        ("1-9", "select"),
    ];
    let quit = "q quit";
    let mut first = true;
    for (key, what) in keys {
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
    spans.push(Span::styled("q", theme::bright()));
    spans.push(Span::styled(" quit", theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::tui::app::tests::{app, press, row};
    use crate::tui::app::{App, Peek};

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
        app.connected = false;
        let text = render(&app, 50, 30);
        assert!(
            text.lines()
                .next()
                .unwrap()
                .contains("serve stopped · r reconnects"),
            "{text}"
        );
        app.connected = true;
        app.hosts.insert("mini".into(), "down".into());
        let text = render(&app, 50, 30);
        assert!(text.lines().next().unwrap().contains("mini down"), "{text}");
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
    fn rows_below_the_list_are_counted() {
        let text = render(&fuller(), 80, 14);
        assert!(text.contains("↓ "), "{text}");
        assert!(text.contains(" more"), "{text}");
    }
}
