//! The turn card: the selected row in full, under the list at every width. In order: who and
//! where with how the turn ended, the headline, the ask or the body, then the pane's tail.

use chrono::{DateTime, Local, Utc};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::app::{App, CardDrawn, Peek, Spot};
use super::markdown::{Block, Run, restyle};
use super::screen::Screen;
use super::theme;
use super::view::key_rows;
use crate::board::Row;
use crate::output::{age, chop, clean, fit, in_state, name, path, state_word};
use crate::pane::bare_title;

/// Text wraps at this width however wide the board, so a line stays readable.
const TEXT: usize = 120;
/// One entry of the key table: the key, a space and what it does.
const KEY_COLUMN: usize = 40;

/// The card's lines for a board `width` wide in `room` rows: the bar, then its text scrolled
/// where ctrl-e and ctrl-y left it, with the tail filling what the text leaves.
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
    let bar = bar(row, right, now, width, false);
    let mut lines = Vec::new();
    let marked = app.marked(row);
    let mut segments = marked.lines.iter();
    match segments.next() {
        None if row.pane.source == "inferred" => {
            lines.push(Line::styled(" no status from this pane", theme::strong()));
            lines.push(Line::styled(
                fit("   space captures the screen", text),
                theme::quiet(),
            ));
        }
        headline => {
            let title = Block::Text(vec![(
                (if title.is_empty() { "-" } else { title }).to_owned(),
                false,
            )]);
            lines.extend(laid_out(
                headline.unwrap_or(&title),
                " ",
                text,
                theme::strong(),
            ));
            for block in segments {
                lines.extend(laid_out(block, "   ", text, theme::plain()));
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
    let window = room.saturating_sub(1);
    let blocked = row.pane.state == "blocked";
    let tail = peek.map_or_else(Vec::new, |peek| {
        let rows = match blocked.then(|| dialog_rows(peek, text)).flatten() {
            // The dialog is the ask, so it is whole however far the card scrolls.
            Some(dialog) => dialog,
            // As much as fits under the ask, with a row left for the facts' `↓` below.
            None if blocked => window.saturating_sub(lines.len() + 1),
            None => window.saturating_sub(lines.len() + meta.len()),
        };
        tail(peek, text, rows)
    });
    lines.extend(tail);
    // A blocked row opens scrolled down to the end of its dialog; any other at the top.
    let anchor = blocked.then_some(lines.len());
    lines.extend(meta);
    let wanted = app
        .card_scroll
        .as_ref()
        .filter(|(key, _)| *key == row.key)
        .map(|(_, at)| *at);
    let (shown, at, last) = scrolled(lines, window, wanted, anchor);
    app.card_drawn.set(CardDrawn {
        at,
        last,
        ..CardDrawn::default()
    });
    std::iter::once(bar).chain(shown).collect()
}

/// `window` rows of `text` from `at`, with a dim `↑ N above` and `↓ N more` where it is cut; and
/// the scroll it shows and the most it can take.
fn scrolled(
    mut text: Vec<Line<'static>>,
    window: usize,
    at: Option<usize>,
    anchor: Option<usize>,
) -> (Vec<Line<'static>>, usize, usize) {
    if text.len() <= window || window < 3 {
        text.truncate(window);
        return (text, 0, 0);
    }
    let last = text.len() - (window - 1);
    // The rows of text that show from `at`, and whether a marker takes the bottom row.
    let fits = |at: usize| {
        let rows = window - usize::from(at > 0);
        if text.len() - at > rows {
            (rows - 1, true)
        } else {
            (rows, false)
        }
    };
    let at = at
        .or_else(|| anchor.map(|end| (0..last).find(|&at| at + fits(at).0 >= end).unwrap_or(last)))
        .unwrap_or(0)
        .min(last);
    let (rows, more) = fits(at);
    let marker = |text: String| Line::styled(text, theme::quiet());
    let mut shown = Vec::new();
    if at > 0 {
        shown.push(marker(format!(" {} {at} above · ctrl-y", theme::ABOVE)));
    }
    let below = text.len() - at - rows;
    shown.extend(text.drain(at..at + rows));
    if more {
        shown.push(marker(format!(" {} {below} more · ctrl-e", theme::MORE)));
    }
    (shown, at, last)
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
pub(crate) fn outcome(row: &Row, now: DateTime<Utc>) -> Option<String> {
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
    marked_lines(&[(text.to_owned(), false)], lead, lead, width, style)
}

/// A message's block on the card under `lead`: text wraps, a list item hangs under its text, a
/// quote keeps `> ` on each row, and code or a table row is cut at the edge with the cut mark.
fn laid_out(block: &Block, lead: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    match block {
        Block::Text(runs) => marked_lines(runs, lead, lead, width, style),
        Block::Item(marker, runs) => {
            let hang = format!("{lead}{}", " ".repeat(marker.width()));
            marked_lines(runs, &format!("{lead}{marker}"), &hang, width, style)
        }
        Block::Quote(runs) => {
            let quoted = format!("{lead}> ");
            marked_lines(runs, &quoted, &quoted, width, style)
        }
        Block::Cut(runs, code) => {
            let lead = format!("{lead}{}", if *code { "  " } else { "" });
            let room = width.saturating_sub(lead.width()).max(1);
            let whole: String = runs.iter().map(|(text, _)| text.as_str()).collect();
            let cut = whole.width() > room;
            let shown = if cut {
                chop(&whole, room.saturating_sub(theme::CUT.width())).swap_remove(0)
            } else {
                whole
            };
            let mut line = styled(&lead, &restyle(&[shown], runs)[0], style);
            if cut {
                line.push_span(Span::styled(theme::CUT, theme::cut_mark(None)));
            }
            vec![line]
        }
        Block::Blank => vec![Line::default()],
    }
}

/// `lead`, then `runs` with strong ones in the strong role, on `style`.
fn styled(lead: &str, runs: &[Run], style: Style) -> Line<'static> {
    let spans = runs.iter().map(|(text, strong)| {
        let role = if *strong {
            style.patch(theme::strong())
        } else {
            style
        };
        Span::styled(text.clone(), role)
    });
    Line::from_iter(std::iter::once(Span::styled(lead.to_owned(), style)).chain(spans))
}

/// As `indented`, with markdown's strong runs in the strong role.
/// `runs` wrapped to `width`, the first row after `first` and the rest after `rest`, both as wide.
fn marked_lines(
    runs: &[Run],
    first: &str,
    rest: &str,
    width: usize,
    style: Style,
) -> Vec<Line<'static>> {
    let room = width.saturating_sub(first.width()).max(1);
    let text: String = runs.iter().map(|(text, _)| text.as_str()).collect();
    let rows: Vec<String> = wrap(&text, room)
        .into_iter()
        .flat_map(|line| chop(&line, room))
        .collect();
    restyle(&rows, runs)
        .iter()
        .enumerate()
        .map(|(n, row)| styled(if n == 0 { first } else { rest }, row, style))
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
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// The rows a message's lines take on the card, `width` wide, as text.
    fn laid(lines: &[&str], width: usize) -> Vec<String> {
        crate::tui::markdown::card(lines)
            .iter()
            .flat_map(|block| laid_out(block, "   ", width, theme::plain()))
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn the_card_lays_out_paragraphs_lists_quotes_code_and_tables_at_40_and_80() {
        let lines = [
            "",
            "First paragraph, long enough to wrap at forty.",
            "",
            "",
            "- an item long enough to wrap at forty",
            "12. twelve wraps four cells in at forty",
            "> a quote that wraps keeps its marker",
            "```sh",
            "  cargo test --workspace --all-features -- --nocapture",
            "```",
            "| a | **b** |",
            "|---|:-:|",
            "| 1 | 2 |",
            "",
        ];
        assert_eq!(
            laid(&lines, 40),
            [
                "   First paragraph, long enough to wrap",
                "   at forty.",
                "",
                "   - an item long enough to wrap at",
                "     forty",
                "   12. twelve wraps four cells in at",
                "       forty",
                "   > a quote that wraps keeps its marker",
                "       cargo test --workspace --all-fea…",
                "   | a | b |",
                "   | 1 | 2 |",
            ]
        );
        assert_eq!(
            laid(&lines, 80),
            [
                "   First paragraph, long enough to wrap at forty.",
                "",
                "   - an item long enough to wrap at forty",
                "   12. twelve wraps four cells in at forty",
                "   > a quote that wraps keeps its marker",
                "       cargo test --workspace --all-features -- --nocapture",
                "   | a | b |",
                "   | 1 | 2 |",
            ],
            "code is cut, never wrapped, and whole when it fits"
        );
    }

    #[test]
    fn nested_items_keep_their_depth_numbers_keep_one_edge_and_tables_align() {
        assert_eq!(
            laid(
                &[
                    "- parent item",
                    "  - child item",
                    "    - grandchild",
                    "- next parent"
                ],
                40
            ),
            [
                "   - parent item",
                "     - child item",
                "       - grandchild",
                "   - next parent",
            ]
        );
        assert_eq!(
            laid(
                &[
                    "1. one",
                    "   - sub of one that wraps under its own text",
                    "2. two"
                ],
                40
            ),
            [
                "   1. one",
                "      - sub of one that wraps under its",
                "        own text",
                "   2. two",
            ]
        );
        assert_eq!(
            laid(
                &["8. eight", "9. nine wraps under its text here", "10. ten"],
                30
            ),
            [
                "    8. eight",
                "    9. nine wraps under its",
                "       text here",
                "   10. ten",
            ]
        );
        assert_eq!(
            laid(
                &[
                    "| crate | before | after |",
                    "|---|:-:|---|",
                    "| **rexboard** | 0.15 | 0.16.0 |",
                    "| 修复 | 1 | 2 |"
                ],
                80
            ),
            [
                "   | crate    | before | after  |",
                "   | rexboard | 0.15   | 0.16.0 |",
                "   | 修复     | 1      | 2      |",
            ],
            "columns pad to their widest cell as it shows"
        );
        let header = &crate::tui::markdown::card(&["| a | b |", "|---|---|", "| c | d |"])[0];
        assert!(
            matches!(header, Block::Cut(runs, false) if runs.iter().any(|(t, s)| *s && t.contains('a'))),
            "the row above the separator is strong: {header:?}"
        );
    }

    #[test]
    fn harness_shows_its_longest_word_whole_at_80_140_and_200() {
        for (view, width) in [
            ("status", 80),
            ("status", 140),
            ("host", 140),
            ("host", 200),
        ] {
            let text = render(&harnesses(view), width, 40);
            assert!(text.contains(" opencode? "), "{view} {width}:\n{text}");
        }
    }

    #[test]
    fn yy_copies_the_message_as_the_agent_sent_it_paragraphs_and_all() {
        let mut app = app();
        let mut rows = rows();
        let msg = "Done. ⏎  ⏎ ```sh ⏎   make ⏎ ``` ⏎ - **next**";
        rows[0].pane.msg = Some(msg.into());
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        assert!(matches!(
            typed(&mut app, "yy").pop().flatten(),
            Some(crate::tui::app::Effect::Copy { text, .. }) if text == msg
        ));
    }

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
    fn a_long_message_scrolls_with_ctrl_e_and_y_and_starts_over_on_another_row() {
        let mut app = fuller();
        let mut rows = app.rows.clone();
        let done = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
        done.pane.msg = Some(format!("Opened PR #31. ⏎ {}", "word ".repeat(400)));
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/claude".into()));
        let text = render(&app, 80, 30);
        let card = card_of(&text);
        assert_eq!(card[1], " Opened PR #31.", "{text}");
        assert!(
            card[2].starts_with("   word word"),
            "the message, whole:\n{text}"
        );
        let more = card.last().unwrap();
        assert!(
            more.starts_with(" ↓ ") && more.ends_with(" more · ctrl-e"),
            "{text}"
        );
        assert!(!text.contains(" where "), "the facts are below:\n{text}");
        let ctrl = |app: &mut App, c: char| {
            key(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
            card_of(&render(app, 80, 30))
        };
        let down = ctrl(&mut app, 'e');
        assert_eq!(down[1], " ↑ 1 above · ctrl-y", "{down:#?}");
        assert!(down[2].starts_with("   word word"), "{down:#?}");
        for _ in 0..100 {
            ctrl(&mut app, 'e');
        }
        let end = ctrl(&mut app, 'e');
        assert!(end[1].starts_with(" ↑ "), "{end:#?}");
        assert!(
            end.last().unwrap().contains("source"),
            "the facts end it, and it stops there: {end:#?}"
        );
        let up = ctrl(&mut app, 'y');
        assert!(up.last().unwrap().ends_with(" more · ctrl-e"), "{up:#?}");
        for k in ['j', 'k'] {
            press(&mut app, KeyCode::Char(k));
            app.note_card(std::time::Instant::now());
        }
        assert_eq!(
            card_of(&render(&app, 80, 30)),
            card,
            "another row starts it over"
        );
    }

    #[test]
    fn tui_json_sets_the_card_height_and_the_wheel_over_the_card_scrolls_it() {
        use crate::tui::prefs::CardHeight;
        let mut app = fuller();
        let mut rows = app.rows.clone();
        let done = rows.iter_mut().find(|r| r.key == "local/claude").unwrap();
        done.pane.msg = Some(format!("Opened PR #31. ⏎ {}", "word ".repeat(400)));
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/claude".into()));
        let bar = |app: &App| {
            let text = render(app, 80, 30);
            text.lines().position(|l| l.starts_with("━━")).unwrap()
        };
        assert_eq!(bar(&app), 30 - 1 - 10, "40% of a 27-row body");
        app.card_height = Some(CardHeight::Rows(12));
        assert_eq!(bar(&app), 30 - 1 - 12);
        app.card_height = Some(CardHeight::Percent(50));
        assert_eq!(bar(&app), 30 - 1 - 13);
        app.card_height = Some(CardHeight::Percent(10));
        assert_eq!(bar(&app), 30 - 1 - 8, "at least 8 rows");
        render(&app, 80, 30);
        let card = app.card_drawn.get();
        app.scroll(card.top + 2, false);
        assert_eq!(card_of(&render(&app, 80, 30))[1], " ↑ 1 above · ctrl-y");
        app.scroll(card.top + 2, true);
        assert_eq!(card_of(&render(&app, 80, 30))[1], " Opened PR #31.");
        app.scroll(card.top - 3, false);
        assert_ne!(
            app.selected,
            Some(Spot::Row("local/claude".into())),
            "over the list it moves the selection"
        );
        app.selected = Some(Spot::Row("local/claude".into()));
        press(&mut app, KeyCode::Char('/'));
        render(&app, 80, 30);
        app.scroll(card.top + 2, false);
        assert_eq!(
            card_of(&render(&app, 80, 30))[1],
            " Opened PR #31.",
            "with a prompt open the wheel leaves the card alone"
        );
    }

    #[test]
    fn the_card_keeps_one_height_for_every_row() {
        let long = (1..=40)
            .map(|n| format!("step {n}"))
            .collect::<Vec<_>>()
            .join(" ⏎ ");
        for (width, height) in [(80u16, 30u16), (140, 40)] {
            let mut tops = Vec::new();
            for app in [fuller(), asking("Bash: ls"), asking(&long)] {
                let mut app = app;
                for _ in 0..4 {
                    let text = render(&app, width, height);
                    let lines: Vec<&str> = text.lines().collect();
                    tops.push(lines.iter().position(|l| l.starts_with("━━")).unwrap());
                    assert!(list_rows(&text) >= 3, "{text}");
                    assert_eq!(
                        lines[lines.len() - 2],
                        "",
                        "{width}: a blank row above the footer"
                    );
                    press(&mut app, KeyCode::Char('j'));
                }
            }
            assert!(
                tops.windows(2).all(|w| w[0] == w[1]),
                "{width}x{height}: {tops:?}"
            );
        }
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
    fn a_blocked_row_opens_at_the_end_of_its_dialog_and_scrolls_up_to_the_rest() {
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
            "Bash: touch zz-probe-file-2",
            "Bash command",
            "Do you want to proceed?",
            "❯ 1. Yes",
            "2. Yes, and always",
            "4. No",
            "Esc to cancel",
        ];
        for (width, height) in [(140u16, 60u16), (200, 60)] {
            let text = render(&app, width, height);
            for line in ask {
                assert!(text.contains(line), "{width}x{height}: {line}:\n{text}");
            }
            let card = card_of(&text);
            assert!(
                card[1].starts_with(" Bash: touch"),
                "{width}x{height}: from the top:\n{text}"
            );
            assert!(
                card.last().unwrap().ends_with(" more · ctrl-e"),
                "{width}x{height}: the facts below:\n{text}"
            );
        }
        let text = render(&app, 80, 30);
        let card = card_of(&text);
        assert!(
            card[1].starts_with(" ↑ ") && card[1].ends_with(" above · ctrl-y"),
            "{text}"
        );
        assert!(
            text.contains("❯ 1. Yes") && text.contains("Esc to cancel"),
            "{text}"
        );
        assert!(
            card.last().unwrap().ends_with(" more · ctrl-e"),
            "the facts below:\n{text}"
        );
        assert!(!text.contains("earlier output"), "only the dialog:\n{text}");
        let mut seen = text;
        for _ in 0..20 {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            );
            seen += &render(&app, 80, 30);
        }
        for line in ask {
            assert!(seen.contains(line), "ctrl-y reaches {line}");
        }
        assert!(!seen.contains("earlier output"), "{seen}");
    }

    #[test]
    fn a_long_blocked_ask_opens_at_its_end_and_the_list_holds_still() {
        let list = list_rows(&render(&fuller(), 80, 30));
        let ask = |lines: usize| {
            (1..=lines)
                .map(|n| format!("line {n} of the question"))
                .collect::<Vec<_>>()
                .join(" ⏎ ")
        };
        let text = render(&asking(&ask(6)), 80, 30);
        for n in 1..=6 {
            assert!(
                text.contains(&format!("line {n} of the question")),
                "{text}"
            );
        }
        assert_eq!(list_rows(&text), list, "{text}");
        let text = render(&asking(&ask(40)), 80, 30);
        assert_eq!(list_rows(&text), list, "{text}");
        let card = card_of(&text);
        assert!(card[1].starts_with(" ↑ "), "{text}");
        assert!(text.contains("line 40 of the question"), "{text}");
        let mut app = asking("Allow it?");
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok((1..=50).map(|n| format!("output {n}")).collect())),
            at: Some(Utc::now()),
            updated_at: None,
        });
        let card = card_of(&render(&app, 80, 30));
        assert_eq!(
            card[1], " Allow it?",
            "no dialog: the ask, then the tail: {card:#?}"
        );
        assert!(card[card.len() - 2].ends_with("output 50"), "{card:#?}");
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
