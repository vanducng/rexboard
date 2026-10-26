//! The turn card: the selected row in full, under the list at every width. In order: who and
//! where with how the turn ended, the headline, the ask or the body, then the pane's tail.

use chrono::{DateTime, Local, Utc};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::{App, Peek, Spot};
use super::screen::Screen;
use super::theme;
use super::view::key_rows;
use crate::board::Row;
use crate::output::{BREAK, age, chop, clean, fit, in_state, name, path, state_word};
use crate::pane::bare_title;

/// Text wraps at this width however wide the board, so a line stays readable.
const TEXT: usize = 120;
/// One entry of the key table: the key, a space and what it does.
const KEY_COLUMN: usize = 40;

/// The card's lines for a board `width` wide, with the tail filling up to `room` lines.
pub fn lines(app: &App, now: DateTime<Utc>, width: usize, room: usize) -> Vec<Line<'static>> {
    if let Some(answer) = &app.answer {
        return answer.lines(now, width, room);
    }
    if app.keys {
        return keys(app, width);
    }
    let Some(row) = app.current() else {
        return match &app.selected {
            Some(Spot::Group(group)) => vec![Line::styled(
                fit(&format!("{} is folded · za opens it", group.title()), width),
                theme::quiet(),
            )],
            _ => Vec::new(),
        };
    };
    let peek = app.peek.as_ref().filter(|peek| peek.key == row.key);
    let title = bare_title(&row.pane.title);
    let text = width.clamp(1, TEXT);
    let sent = app
        .sent
        .as_ref()
        .filter(|sent| sent.key == row.key)
        .map(|sent| sent.text.as_str());
    let right = sent.map(str::to_owned).or_else(|| fresh(row, peek, now));
    let mut lines = vec![bar(row, right, now, width, false)];
    let msg = clean(row.pane.msg.as_deref().unwrap_or_default());
    let mut segments = msg.split(BREAK).map(str::trim).filter(|s| !s.is_empty());
    match segments.next() {
        None if row.pane.source == "inferred" => {
            lines.push(Line::styled(" no status from this pane", theme::strong()));
            lines.push(Line::styled(
                fit("   space captures the screen", text),
                theme::quiet(),
            ));
        }
        headline => {
            let headline = headline.unwrap_or(if title.is_empty() { "-" } else { title });
            lines.extend(indented(headline, " ", text, theme::strong()));
            for segment in segments {
                lines.extend(indented(segment, "   ", text, theme::plain()));
            }
        }
    }
    if let Some(why) = &app.failed {
        lines.extend(indented(why, " ", text, theme::alert()));
    }
    if let Some(why) = &row.suspect_reason {
        lines.extend(indented(
            &format!("may be stale: {why}"),
            " ",
            text,
            theme::quiet(),
        ));
    }
    let mut meta = super::meta::lines(app, row, text);
    meta.insert(0, Line::default());
    // A blocked row's ask and tail are what it asks, so they are never cut for the facts; the
    // facts take the rows left. Any other message gives way to the facts, down to its headline.
    let blocked = row.pane.state == "blocked";
    if room > 0 && !blocked {
        let over = (lines.len() + meta.len()).saturating_sub(room);
        shorten(&mut lines, over, text);
    }
    let tail = peek.map_or_else(Vec::new, |peek| {
        let mut rows = room.saturating_sub(lines.len());
        if !blocked {
            rows = rows.saturating_sub(meta.len());
        } else if let Some(dialog) = dialog_rows(peek, text) {
            // The dialog is the ask; the output above it gives way to the facts.
            rows = rows.min(dialog);
        }
        tail(peek, text, rows)
    });
    if room > 0 {
        let left = room.saturating_sub(lines.len() + tail.len());
        // The blank line alone is no facts.
        meta.truncate(if left < 2 { 0 } else { left });
    }
    lines.extend(tail);
    lines.extend(meta);
    lines
}

/// The rows a tail's dialog takes `width` wide, from its opening to the end, as `tail` draws
/// them; None when the tail shows no dialog.
fn dialog_rows(peek: &Peek, width: usize) -> Option<usize> {
    let Some(Ok(lines)) = &peek.lines else {
        return None;
    };
    // The parse drops blank lines at the end, which the tail still draws.
    let blank = lines
        .iter()
        .rev()
        .take_while(|l| l.trim().is_empty())
        .count();
    let count = Screen::parse(&lines.join("\n"), None).dialog? + blank;
    let room = width.saturating_sub(theme::GUTTER.width()).max(1);
    Some(
        lines[lines.len().saturating_sub(count)..]
            .iter()
            .map(|line| chop(&clean(line), room).len())
            .sum(),
    )
}

/// The message `over` lines shorter, keeping the bar and the headline; the last kept line ends
/// in `…`.
fn shorten(lines: &mut Vec<Line<'static>>, over: usize, width: usize) {
    let keep = lines.len().saturating_sub(over).max(2);
    if keep >= lines.len() {
        return;
    }
    lines.truncate(keep);
    if let Some(last) = lines.last_mut() {
        let text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        let style = last.spans.first().map_or(last.style, |s| s.style);
        *last = Line::styled(
            fit(&format!("{}…", text.trim_end()), width)
                .trim_end()
                .to_owned(),
            style,
        );
    }
}

/// The first line: the row's state, its name and how long it has been so, then how fresh the
/// tail is, or the answer the board last sent it. The full view, which has no list or facts
/// beside it, also names where the row is and its host.
/// `━━ ✓ done · seat-a · ended 4m ago ━━━━━━ captured …`
pub(super) fn bar(
    row: &Row,
    right: Option<String>,
    now: DateTime<Utc>,
    width: usize,
    placed: bool,
) -> Line<'static> {
    let mut state = state_word(row);
    if let Some(kind) = &row.pane.kind {
        state = format!("{state} · {kind}");
    }
    let ends = theme::BAR_END;
    let right = right.map_or_else(|| ends.to_owned(), |text| format!(" {text} {ends}"));
    let room = width.saturating_sub(ends.width() + right.width() + 1);
    let line = |place: Option<&str>, extra: &[String]| {
        let mut parts = vec![name(row)];
        parts.extend(place.map(str::to_owned));
        parts.extend_from_slice(extra);
        parts.extend(outcome(row, now));
        clean(&format!(
            " {} {state} · {} ",
            theme::glyph(row, now),
            parts.join(" · ")
        ))
    };
    let left = if placed {
        // On a narrow bar the host gives way first, then the place, before the outcome.
        let labels = [row.pane.session.as_str(), row.pane.tab.as_str()];
        let least = path(&labels, usize::MAX).width().min(12);
        let mut extra = vec![row.pane.host.clone()];
        while !extra.is_empty() && line(Some(""), &extra).width() + least > room {
            extra.pop();
        }
        match room.checked_sub(line(Some(""), &extra).width()) {
            Some(place) if place >= least => line(Some(&path(&labels, place)), &extra),
            _ => line(None, &extra),
        }
    } else {
        line(None, &[])
    };
    let left = fit(&left, room).trim_end().to_owned() + " ";
    let fill = width.saturating_sub(ends.width() + left.width() + right.width());
    let mut spans = vec![Span::styled(ends, theme::quiet())];
    // The glyph keeps its own face: in a font with no bold braille a bold spinner is a still blob.
    let (glyph, bold) = (theme::glyph(row, now), theme::bar_label(row));
    match left.strip_prefix(&format!(" {glyph}")) {
        Some(label) => spans.extend([
            Span::styled(" ", bold),
            Span::styled(glyph, theme::state(row)),
            Span::styled(label.to_owned(), bold),
        ]),
        None => spans.push(Span::styled(left, bold)),
    }
    spans.extend([
        Span::styled(theme::RULE.repeat(fill), theme::quiet()),
        Span::styled(right, theme::quiet()),
    ]);
    Line::from(spans)
}

/// How the turn stands: `ended 4m ago`, `waiting 12s`, `for 2m`. Nothing for a
/// row with no report to date it.
fn outcome(row: &Row, now: DateTime<Utc>) -> Option<String> {
    let since = in_state(row, now);
    if since == "-" {
        return None;
    }
    Some(match row.pane.state.as_str() {
        "done" => format!("ended {since} ago"),
        "blocked" => format!("waiting {since}"),
        "error" => format!("failed {since} ago"),
        _ => format!("for {since}"),
    })
}

/// `captured 11:21:14 · 8s` for a tail, or `newer report · space refreshes` once the row has
/// reported since the tail was taken.
fn fresh(row: &Row, peek: Option<&Peek>, now: DateTime<Utc>) -> Option<String> {
    let peek = peek?;
    let Some(at) = peek.at else {
        return Some("capturing…".to_owned());
    };
    let parsed = |at: Option<&str>| at.and_then(|at| DateTime::parse_from_rfc3339(at).ok());
    if let (Some(report), Some(asked)) = (
        parsed(row.pane.updated_at.as_deref()),
        parsed(peek.updated_at.as_deref()),
    ) && report > asked
    {
        return Some("newer report · space refreshes".to_owned());
    }
    Some(format!(
        "captured {} · {}",
        at.with_timezone(&Local).format("%H:%M:%S"),
        age(&at.to_rfc3339(), now)
    ))
}

/// `text` wrapped to `width` after `lead`, the lines past the first under it; a word longer than
/// the line is cut into pieces, so nothing runs off the card.
fn indented(text: &str, lead: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let room = width.saturating_sub(lead.width()).max(1);
    wrap(text, room)
        .into_iter()
        .flat_map(|line| chop(&line, room))
        .map(|line| Line::styled(format!("{lead}{line}"), style))
        .collect()
}

/// The newest `room` rows of the tail, wrapped, so the last lines show whole and the oldest fall
/// off the top. A gutter marks them as the pane's own screen, not the agent's report.
pub fn tail(peek: &Peek, width: usize, room: usize) -> Vec<Line<'static>> {
    match &peek.lines {
        None => vec![Line::styled(" capturing…", theme::quiet())],
        Some(Ok(text)) => {
            let mut rows = Vec::new();
            for line in text.iter().rev() {
                if rows.len() >= room {
                    break;
                }
                rows.extend(
                    // The full view's reads keep the pane's colours, which the card drops.
                    chop(
                        &clean(line),
                        width.saturating_sub(theme::GUTTER.width()).max(1),
                    )
                    .into_iter()
                    .rev(),
                );
            }
            rows.truncate(room);
            rows.into_iter()
                .rev()
                .map(|line| Line::styled(format!("{}{line}", theme::GUTTER), theme::quiet()))
                .collect()
        }
        // An error is worth reading whole, so it wraps instead of being cut.
        Some(Err(why)) => indented(why, " ", width, theme::alert()),
    }
}

/// The `?` table, in as many columns as fit, filled down each column.
fn keys(app: &App, width: usize) -> Vec<Line<'static>> {
    let all = key_rows(app);
    let columns = (width / KEY_COLUMN).max(1);
    let rows = all.len().div_ceil(columns);
    let motion = " · \"motion\": \"reduced\" in tui.json stills the spinner";
    let title = if " keys".width() + motion.width() <= width {
        Line::from(vec![
            Span::styled(" keys", theme::accent()),
            Span::styled(motion, theme::quiet()),
        ])
    } else {
        Line::styled(fit(" keys", width), theme::accent())
    };
    let mut lines = vec![title];
    for r in 0..rows {
        let mut spans = Vec::new();
        for c in 0..columns {
            let Some((key, what)) = all.get(c * rows + r) else {
                break;
            };
            spans.push(Span::styled(format!(" {key:<8} "), theme::strong()));
            let room = KEY_COLUMN.min(width).saturating_sub(10);
            spans.push(Span::styled(fit(what, room), theme::quiet()));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Words joined into lines of at most `width` columns.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    #[test]
    fn the_card_says_who_where_how_and_what_at_80_140_and_200() {
        let app = asking("Bash: git push origin main --force-with-lease");
        for (width, height) in [(80u16, 30u16), (140, 30), (200, 40), (140, 60)] {
            let text = render(&app, width, height);
            let card = card_of(&text);
            assert!(
                card[0].starts_with("━━ ! blocked · permission · seat-b · waiting 5h43m ━"),
                "{width}: the bar says only what, who and how long:\n{text}"
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
            let words = |l: &String| l.split_whitespace().collect::<Vec<_>>().join(" ");
            let facts: Vec<String> = card[2..].iter().map(words).collect();
            assert_eq!(
                facts,
                [
                    "",
                    "where api › main › seat-b",
                    "host mini harness claude",
                    "title seat-b title source native"
                ],
                "{width}: one blank line, then the facts, where first, each once:\n{text}"
            );
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(
                words(&lines[lines.len() - 2].to_owned()),
                "title seat-b title source native",
                "{width}: the card sits on the footer:\n{text}"
            );
            assert!(
                lines[lines.len() - 8].trim().is_empty(),
                "{width}: the room it does not need opens above it:\n{text}"
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
            card[0].contains(" · ended 5h43m ago ━") && !card[0].contains("turn"),
            "{}",
            card[0]
        );
        assert!(
            card.iter()
                .any(|l| l.starts_with(" host ") && l.trim_end().ends_with("turn    6m")),
            "the turn is a fact: {text}"
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
    fn a_long_message_gives_way_to_the_facts() {
        let mut app = fuller();
        let mut rows = app.rows.clone();
        let done = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
        done.pane.msg = Some(format!("Opened PR #31. ⏎ {}", "word ".repeat(400)));
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/claude".into()));
        let text = render(&app, 80, 30);
        let lines: Vec<&str> = text.lines().collect();
        let place = lines
            .iter()
            .position(|l| l.starts_with(" where "))
            .expect(&text);
        assert!(lines[place - 1].trim().is_empty(), "{text}");
        assert!(
            lines[place - 2].ends_with("word…"),
            "the cut is marked:\n{text}"
        );
        assert!(
            lines[lines.len() - 2].contains("source"),
            "the facts end the card:\n{text}"
        );
    }

    #[test]
    fn the_facts_share_rows_when_wide_and_take_one_each_when_narrow() {
        let app = asking("Bash: ls");
        let facts = |width: u16| -> Vec<String> {
            let text = render(&app, width, 30);
            let card = card_of(&text);
            let blank = card.iter().position(|l| l.trim().is_empty()).unwrap();
            card[blank + 1..]
                .iter()
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect()
        };
        assert_eq!(
            facts(80),
            [
                "where api › main › seat-b",
                "host mini harness claude",
                "title seat-b title source native"
            ]
        );
        assert_eq!(
            facts(36),
            [
                "where api › main › seat-b",
                "host mini",
                "harness claude",
                "title seat-b title",
                "source native"
            ]
        );
    }

    #[test]
    fn nerd_icons_label_each_fact_with_one_cell_at_60_100_and_140() {
        let mut app = asking("Bash: ls");
        let private = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
        for nerd in [false, true] {
            app.nerd = nerd;
            for width in [60u16, 100, 140] {
                let text = render(&app, width, 30);
                assert!(
                    text.lines().all(|l| l.width() <= usize::from(width)),
                    "{width}:\n{text}"
                );
                let card = card_of(&text);
                let blank = card.iter().position(|l| l.trim().is_empty()).unwrap();
                let facts = &card[blank + 1..];
                if !nerd {
                    assert!(facts[0].starts_with(" where "), "{width}:\n{text}");
                    assert!(!text.chars().any(private), "{width}: words only");
                    continue;
                }
                for line in facts {
                    let mut chars = line.chars();
                    assert_eq!(chars.next(), Some(' '), "{line}");
                    assert!(chars.next().is_some_and(private), "a glyph leads: {line}");
                    assert_eq!(chars.next(), Some(' '), "one cell, then the value: {line}");
                }
                let place = facts.iter().find(|l| l.contains(" › ")).unwrap();
                assert_eq!(
                    place.chars().filter(|c| private(*c)).count(),
                    3,
                    "the session, window and block each lead: {place}"
                );
                assert!(place.ends_with("seat-b"), "{place}");
                assert!(!text.contains(" host "), "{width}: no word labels:\n{text}");
            }
        }
    }

    #[test]
    fn a_blocked_rows_tail_keeps_the_whole_dialog_and_the_facts_take_what_is_left() {
        let mut app = asking("Bash: touch zz-probe-file-2");
        let screen = include_str!("../../tests/fixtures/screens/claude-permission.txt");
        // A real pane: plenty of earlier output, then the rule Claude draws above its dialog.
        let mut lines: Vec<String> = (1..=200).map(|n| format!("earlier output {n}")).collect();
        lines.push("─".repeat(80));
        lines.extend(screen.lines().map(str::to_owned));
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(lines)),
            at: Some(Utc::now()),
            updated_at: None,
        });
        let ask = [
            "Do you want to proceed?",
            "❯ 1. Yes",
            "2. Yes, and always",
            "4. No",
            "Esc to cancel",
        ];
        for (width, height, facts) in [(80u16, 30u16, false), (140, 60, true), (200, 60, true)] {
            let text = render(&app, width, height);
            for line in ask {
                assert!(text.contains(line), "{width}x{height}: {line}:\n{text}");
            }
            // A 30-row card has room for the question and its options, as before the facts;
            // a taller one shows the dialog from its opening.
            assert_eq!(
                text.contains("Bash command"),
                facts,
                "{width}x{height}:\n{text}"
            );
            assert!(
                !text.contains("earlier output"),
                "{width}x{height}: only the dialog:\n{text}"
            );
            assert_eq!(
                text.contains(" where "),
                facts,
                "{width}x{height}: the facts take the rows left:\n{text}"
            );
        }
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
    fn a_long_window_gives_way_in_where_and_the_block_never() {
        let mut app = asking("Bash: ls");
        let mut rows = app.rows.clone();
        rows[0].pane.tab = "2-toolrig:rig:demo-team#fedcba9876543210#l2".into();
        app.set_rows(rows, hosts());
        let text = render(&app, 80, 30);
        let place = text
            .lines()
            .find(|l| l.starts_with(" where "))
            .expect(&text);
        assert!(
            place.contains("2-toolrig:ri…") && place.ends_with(" › seat-b"),
            "the window gives way in the middle, the block never: {place}"
        );
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(vec!["tail line".into()])),
            at: Some(Utc::now()),
            updated_at: None,
        });
        let text = render(&app, 80, 30);
        let bar = &card_of(&text)[0];
        assert!(
            bar.starts_with("━━ ! blocked · permission · seat-b · waiting ")
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
    fn the_bar_draws_its_glyph_in_the_state_face_and_only_the_label_bold() {
        use ratatui::style::Modifier;
        let now = DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let app = every_state();
        for key in ["local/busy", "local/allow"] {
            let row = app.rows.iter().find(|row| row.key == key).unwrap();
            let bar = crate::tui::card::bar(row, None, now, 80, false);
            let glyph = theme::glyph(row, now);
            let mark = bar.spans.iter().find(|s| s.content == glyph).unwrap();
            assert_eq!(mark.style, theme::state(row), "{key}: the list's face");
            let label = bar
                .spans
                .iter()
                .find(|s| s.content.contains(" · "))
                .unwrap();
            assert!(label.style.add_modifier.contains(Modifier::BOLD), "{key}");
        }
    }
}
