//! The full view (`space`): one row's pane across the whole terminal, read again every couple of
//! seconds, its lines as the pane laid them out.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Peek};
use super::{card, theme, vt};
use crate::board::Row;
use crate::output::{age, chop, fit};

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
            true,
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
            Some(why) => Line::styled(fit(&format!(" {why}"), width), theme::alert()),
            None => Line::styled(" capturing…", theme::quiet()),
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
        // On the row's own background, so a coloured band has no notch at the cut.
        let bg = kept.last().and_then(|span| span.style.bg);
        kept.push(Span::styled(theme::CUT, theme::cut_mark(bg)));
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
    // The footer's keys, most needed first; a narrow terminal drops them from the end.
    let listed = super::keys::FULL
        .iter()
        .filter(|entry| !entry.keys.is_empty())
        .map(|entry| (entry.keys, entry.word));
    for (key, what) in listed {
        let sep = if spans.is_empty() { "" } else { " · " };
        let piece = sep.width() + key.width() + 1 + what.width();
        if used + piece > width {
            break;
        }
        used += piece;
        spans.push(Span::styled(sep, theme::quiet()));
        spans.push(Span::styled(key, theme::strong()));
        spans.push(Span::styled(format!(" {what}"), theme::quiet()));
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
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    use super::*;
    use crate::tui::app::testkit::*;
    use crate::tui::app::{Effect, Spot};

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

    const DIFF: [&str; 3] = [
        "\x1b[31m- removed\x1b[0m",
        "\x1b[32m+ added\x1b[0m \x1b[1;3;4;7mloud\x1b[22;23;24;27m quiet",
        "\x1b[38;5;244m@@ -1 +1 @@\x1b[0m   ",
    ];

    #[test]
    fn full_view_draws_the_panes_colours_and_modifiers() {
        let buffer = full_cells(&DIFF, 80);
        assert_eq!(row_text(&buffer, 1), "- removed");
        assert_eq!(row_text(&buffer, 2), "+ added loud quiet");
        assert_eq!(
            row_text(&buffer, 3),
            "@@ -1 +1 @@",
            "padding after the text is dropped"
        );
        assert_eq!(buffer[(0, 1)].fg, Color::Indexed(1));
        assert_eq!(buffer[(2, 2)].fg, Color::Indexed(2));
        let loud = buffer[(8, 2)].modifier;
        for m in [
            Modifier::BOLD,
            Modifier::ITALIC,
            Modifier::UNDERLINED,
            Modifier::REVERSED,
        ] {
            assert!(loud.contains(m), "{m:?} in {loud:?}");
        }
        assert!(buffer[(13, 2)].modifier.is_empty(), "turned off again");
        assert_eq!(buffer[(13, 2)].fg, Color::Reset, "reset by 0 before it");
        assert_eq!(buffer[(0, 3)].fg, Color::Indexed(244));
    }

    #[test]
    fn full_view_cuts_a_coloured_line_at_80_without_splitting_its_escapes() {
        let long = format!("\x1b[48;2;0;64;0m+ {}\x1b[0m\x1b[31mtail", "x".repeat(120));
        let buffer = full_cells(&[&long], 80);
        let text = row_text(&buffer, 1);
        assert_eq!(text.chars().count(), 80);
        assert!(text.starts_with("+ xxx") && text.ends_with("x…"), "{text}");
        assert!(!text.contains('\x1b') && !text.contains("[0m"), "{text}");
        assert_eq!(buffer[(0, 1)].bg, Color::Rgb(0, 64, 0));
        assert_eq!(
            buffer[(78, 1)].bg,
            Color::Rgb(0, 64, 0),
            "the colour runs to the cut"
        );
        assert_eq!(buffer[(79, 1)].symbol(), "…");
        assert!(buffer[(79, 1)].modifier.contains(Modifier::DIM));
        assert_eq!(
            buffer[(79, 1)].bg,
            Color::Rgb(0, 64, 0),
            "the mark sits on the row's band, with no notch"
        );
        let plain = full_cells(&[&format!("{} end", "x".repeat(120))], 80);
        assert_eq!(
            plain[(79, 1)].bg,
            Color::Reset,
            "an uncoloured row's mark has no background"
        );
        let padded = format!("\x1b[48;2;0;64;0m+ added{}\x1b[0m", " ".repeat(150));
        let buffer = full_cells(&[&padded], 80);
        assert_eq!(
            row_text(&buffer, 1),
            "+ added",
            "no mark when only padding is past the edge"
        );
        assert_eq!(
            buffer[(79, 1)].bg,
            Color::Rgb(0, 64, 0),
            "the background runs to the edge"
        );
        let wide = full_cells(&[&long], 200);
        assert!(row_text(&wide, 1).ends_with("xtail"), "uncut when it fits");
        assert_eq!(wide[(122, 1)].fg, Color::Indexed(1));
    }

    #[test]
    fn full_view_reads_with_colours_and_the_card_shows_them_plain() {
        let mut app = fuller();
        let effect = press(&mut app, KeyCode::Char(' '));
        assert!(
            matches!(effect, Some(Effect::Peek { vt: true, .. })),
            "{effect:?}"
        );
        app.peeked(
            "mini/seat-b",
            Ok(DIFF.iter().map(|l| (*l).to_owned()).collect()),
        );
        let t0 = app.full.as_ref().unwrap().asked;
        assert!(matches!(
            app.live(t0 + crate::tui::full::EVERY),
            Some(Effect::Peek { vt: true, .. })
        ));
        press(&mut app, KeyCode::Esc);
        let text = render(&app, 80, 30);
        assert!(
            text.contains("- removed") && text.contains("+ added loud quiet"),
            "{text}"
        );
        assert!(!text.contains('\x1b') && !text.contains("[31m"), "{text}");
        let mut board = fuller();
        board.selected = Some(Spot::Row("mini/seat-b".into()));
        let t = Instant::now();
        board.note_card(t);
        let tail = board.auto_tail(t + Duration::from_secs(1));
        assert!(
            matches!(tail, Some(Effect::Peek { vt: false, .. })),
            "the card's own tail stays text: {tail:?}"
        );
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
                80 => "space back · j/k scroll · a answer · enter jump · gg/G top, bottom",
                _ => {
                    "space back · j/k scroll · a answer · enter jump · gg/G top, bottom · ctrl-d/u half page · ctrl-f/b page"
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
        crate::tui::app::testkit::key(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            render(&app, 80, 30).lines().nth(28),
            Some("peek line 143"),
            "ctrl-b pages too"
        );
        crate::tui::app::testkit::key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            render(&app, 80, 30).lines().nth(28),
            Some("peek line 157"),
            "ctrl-d half a page"
        );
        crate::tui::app::testkit::key(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            render(&app, 80, 30).lines().nth(28),
            Some("peek line 156"),
            "ctrl-y a line"
        );
        press(&mut app, KeyCode::Char('g'));
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
    fn full_view_keys_answer_jump_go_back_and_leave_the_board_keys_alone() {
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
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle(ctrl_c), Some(Effect::Quit));
        assert_eq!(press(&mut app, KeyCode::Char('q')), None, "q goes back");
        assert!(app.full.is_none());
        assert_eq!(
            press(&mut app, KeyCode::Char('q')),
            Some(Effect::Quit),
            "then quits"
        );
        press(&mut app, KeyCode::Char(' '));
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
}
