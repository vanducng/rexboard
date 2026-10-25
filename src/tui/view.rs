use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Item};
use super::theme;
use crate::board::Row;
use crate::output::{fit, in_state, place, squeeze, what};

/// Width of the detail pane when it sits on the right.
const DETAIL: u16 = 38;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Tier {
    /// Under 60 columns: glyph, time in state, pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state word and `session › tab`, with the detail pane on the right.
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
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
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
        detail(frame, padded, app, now);
    } else {
        let [list_area, rule, detail_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(9),
        ])
        .areas(body);
        list(frame, list_area, app, tier, now);
        frame.render_widget(
            Paragraph::new("─".repeat(usize::from(rule.width))).style(theme::dim()),
            rule,
        );
        detail(frame, detail_area, app, now);
    }
    footer(frame, bottom, app);
}

fn header(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let agents = app.rows.len();
    let left = vec![
        Span::styled("rexboard", theme::accent()),
        Span::styled(
            format!(
                " · {} hosts · {} panes · {agents} agents",
                app.hosts.len(),
                app.panes
            ),
            theme::dim(),
        ),
    ];
    let clock = now.with_timezone(&Local).format("%H:%M").to_string();
    let mut right = vec![Span::styled(clock, theme::dim())];
    if !app.connected {
        right.push(Span::styled(
            " · serve stopped, r reconnects",
            Style::new().fg(Color::Red),
        ));
    } else {
        right.push(Span::styled(" · ", theme::dim()));
        for (i, (host, state)) in app.hosts.iter().enumerate() {
            if i > 0 {
                right.push(Span::styled(" + ", theme::dim()));
            }
            let style = if state == "up" {
                theme::dim()
            } else {
                Style::new().fg(Color::Red)
            };
            let label = if state == "up" {
                host.clone()
            } else {
                format!("{host} {state}")
            };
            right.push(Span::styled(label, style));
        }
    }
    let used = |spans: &[Span]| spans.iter().map(|s| s.content.width()).sum::<usize>();
    let width = usize::from(area.width);
    if used(&left) + used(&right) + 1 > width {
        right.truncate(1);
    }
    let gap = width.saturating_sub(used(&left) + used(&right));
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn list(frame: &mut Frame, area: Rect, app: &App, tier: Tier, now: DateTime<Utc>) {
    let items = app.items();
    if items.is_empty() {
        let text = if app.connected {
            "no agent panes yet"
        } else {
            "waiting for serve"
        };
        frame.render_widget(Paragraph::new(Line::styled(text, theme::dim())), area);
        return;
    }
    let height = usize::from(area.height);
    let at = items.iter().position(|item| match item {
        Item::Row(i) => app.selected.as_deref() == Some(app.rows[*i].key.as_str()),
        Item::Header(..) => false,
    });
    let offset = at.map_or(0, |at| (at + 1).saturating_sub(height));
    let width = usize::from(area.width);
    let lines: Vec<Line> = items
        .iter()
        .skip(offset)
        .take(height)
        .map(|item| match *item {
            Item::Header(group, count) => {
                let title = format!("{} ({count}) ", theme::group_title(group));
                let fill = "┄".repeat(width.saturating_sub(title.width()));
                Line::styled(format!("{title}{fill}"), theme::header())
            }
            Item::Row(i) => {
                let row = &app.rows[i];
                let selected = app.selected.as_deref() == Some(row.key.as_str());
                row_line(row, tier, width, selected, now)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn harness(row: &Row) -> String {
    let name = match row.pane.harness.as_deref() {
        Some("claude-code") => "claude",
        Some(other) => other,
        None => "-",
    };
    if row.pane.source == "inferred" && row.pane.harness.is_some() {
        format!("{name}?")
    } else {
        name.to_owned()
    }
}

fn row_line(
    row: &Row,
    tier: Tier,
    width: usize,
    selected: bool,
    now: DateTime<Utc>,
) -> Line<'static> {
    let inferred = row.pane.source == "inferred";
    let soft = if inferred || row.stale {
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
    let place_text = if tier == Tier::Wide {
        place(row)
    } else {
        squeeze(&row.pane.pane, 24)
    };
    let place_width = if tier == Tier::Wide {
        rest * 5 / 9
    } else {
        rest * 2 / 5
    };
    cells.push((fit(&place_text, place_width.saturating_sub(1)) + " ", soft));
    cells.push((fit(what(row), rest.saturating_sub(place_width)), soft));
    let mut spans: Vec<Span> = cells
        .into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect();
    if row.stale {
        for span in &mut spans {
            span.style = span.style.patch(theme::dim());
        }
    }
    if selected {
        for span in &mut spans {
            span.style = span.style.bg(theme::RAISED);
        }
    }
    Line::from(spans).style(if selected {
        Style::new().bg(theme::RAISED)
    } else {
        Style::new()
    })
}

fn detail(frame: &mut Frame, area: Rect, app: &App, now: DateTime<Utc>) {
    let Some(row) = app.current() else {
        return;
    };
    let width = usize::from(area.width);
    let value = width.saturating_sub(10);
    let p = &row.pane;
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
    let msg = p
        .msg
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "-".into());
    // Most useful first: underneath the list only the first few lines fit.
    let pairs = [
        ("state", state),
        ("in state", in_state(row, now)),
        ("msg", msg),
        ("host", p.host.clone()),
        ("session", p.session.clone()),
        ("tab", p.tab.clone()),
        ("title", p.title.clone()),
        ("harness", p.harness.clone().unwrap_or_else(|| "-".into())),
        ("source", p.source.clone()),
        ("last rpt", reported),
    ];
    let mut lines = vec![Line::styled(fit(&p.pane, width), theme::accent())];
    for (key, val) in pairs {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:<9} "), theme::dim()),
            Span::raw(fit(&val, value)),
        ]));
    }
    if p.source == "inferred" {
        lines.push(Line::styled(
            fit("no status record; state from the title glyph", width),
            theme::dim(),
        ));
    }
    lines.push(Line::styled(
        fit("enter jumps in a later version", width),
        theme::dim(),
    ));
    if let Some(peek) = app.peek.as_ref().filter(|peek| peek.key == row.key) {
        lines.push(Line::styled("peek", theme::header()));
        match &peek.lines {
            None => lines.push(Line::styled("capturing…", theme::dim())),
            Some(text) => lines.extend(text.iter().map(|l| Line::raw(fit(l, width)))),
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
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
    let keys = [
        ("j/k", "move"),
        ("enter", "jump"),
        ("space", "peek"),
        ("s", "seen"),
        ("/", "filter"),
        ("1-9", "select"),
    ];
    let quit = "q quit";
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for (i, (key, what)) in keys.iter().enumerate() {
        let sep = if i == 0 { "" } else { " · " };
        let piece = sep.width() + key.width() + 1 + what.width();
        if used + piece + quit.width() + 3 > width {
            break;
        }
        spans.push(Span::styled(sep, theme::dim()));
        spans.push(Span::styled(*key, theme::bright()));
        spans.push(Span::styled(format!(" {what}"), theme::dim()));
        used += piece;
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

    use super::*;
    use crate::tui::app::tests::{app, row};

    fn screen(width: u16, height: u16) -> String {
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
        let now = DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, &app, now)).unwrap();
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
        assert!(seat_line(&mid).contains("claude"), "{mid}");
        assert!(seat_line(&mid).contains("mini"), "{mid}");
        assert!(seat_line(&wide).contains("blocked"), "{wide}");
        assert!(seat_line(&wide).contains("api › main › seat-b"), "{wide}");
        assert!(wide.contains("│"), "the detail pane is on the right at 140");
        assert!(narrow.contains("─────"), "and underneath below 100");
        assert!(
            wide.contains("2-openrig:ri…89abcdef#l2"),
            "long tab labels are squeezed:\n{wide}"
        );
    }

    #[test]
    fn a_row_with_no_record_shows_a_dash_for_its_age() {
        let wide = screen(140, 40);
        let line = wide.lines().find(|l| l.contains("inferred msg")).unwrap();
        assert!(line.contains(" -  "), "{line}");
    }
}
