//! The inline markdown an agent's message carries, as runs of text in the plain or strong role:
//! `**bold**`, `__bold__` and `` `code` `` are strong, `[text](url)` is its text, and the markers
//! go. Italics stay as written, so `snake_case` and `2*3*4` read as they are.

/// A run of text, and whether it takes the strong role.
pub type Run = (String, bool);

/// One line's inline markdown. `text` is already free of control and escape sequences.
pub fn inline(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            let ticks = chars[i..].iter().take_while(|&&t| t == '`').count();
            if let Some(end) = ticks_at(&chars, i + ticks, ticks) {
                push(
                    &mut runs,
                    &chars[i + ticks..end].iter().collect::<String>(),
                    true,
                );
                i = end + ticks;
                continue;
            }
            push(&mut runs, &"`".repeat(ticks), false);
            i += ticks;
            continue;
        }
        if (c == '*' || c == '_')
            && chars.get(i + 1) == Some(&c)
            && opens(&chars, i, c)
            && let Some(end) = closes(&chars, i + 2, c)
        {
            let inner: String = chars[i + 2..end].iter().collect();
            for (text, _) in inline(&inner) {
                push(&mut runs, &text, true);
            }
            i = end + 2;
            continue;
        }
        if c == '['
            && let Some(close) = (i + 1..chars.len()).find(|&j| chars[j] == ']')
            && chars.get(close + 1) == Some(&'(')
            && let Some(paren) = (close + 2..chars.len()).find(|&j| chars[j] == ')')
        {
            for (text, strong) in inline(&chars[i + 1..close].iter().collect::<String>()) {
                push(&mut runs, &text, strong);
            }
            i = paren + 1;
            continue;
        }
        push(&mut runs, &c.to_string(), false);
        i += 1;
    }
    runs
}

fn push(runs: &mut Vec<Run>, text: &str, strong: bool) {
    match runs.last_mut() {
        Some((last, was)) if *was == strong => last.push_str(text),
        _ if text.is_empty() => {}
        _ => runs.push((text.to_owned(), strong)),
    }
}

/// Where a run of exactly `n` backticks starts, from `from` on.
fn ticks_at(chars: &[char], from: usize, n: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        let run = chars[j..].iter().take_while(|&&t| t == '`').count();
        if run == n {
            return Some(j);
        }
        j += run.max(1);
    }
    None
}

/// A pair of `c` at `i` that can open: not followed by a space, and for `__` not inside a word.
fn opens(chars: &[char], i: usize, c: char) -> bool {
    let next = chars.get(i + 2).is_some_and(|n| !n.is_whitespace());
    next && !(c == '_' && i > 0 && chars[i - 1].is_alphanumeric())
}

/// The pair of `c` that closes one opened before `from`: not after a space, and for `__` not
/// inside a word.
fn closes(chars: &[char], from: usize, c: char) -> Option<usize> {
    (from + 1..chars.len().saturating_sub(1)).find(|&j| {
        chars[j] == c
            && chars[j + 1] == c
            && !chars[j - 1].is_whitespace()
            && !(c == '_' && chars.get(j + 2).is_some_and(|n| n.is_alphanumeric()))
    })
}

/// A line without the block markers it leads with: a heading's `#`s, a quote's `>`, a list's
/// `-`, `*`, `+` or `1.`, and a code fence.
pub fn unblock(line: &str) -> &str {
    let mut line = line.trim_start();
    loop {
        let hashes = line.len() - line.trim_start_matches('#').len();
        let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let rest = if let Some(rest) = line.strip_prefix("```") {
            Some(rest)
        } else if hashes > 0 && line[hashes..].starts_with(' ') {
            Some(&line[hashes..])
        } else if let Some(rest) = line.strip_prefix('>') {
            Some(rest)
        } else if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|m| line.strip_prefix(m)) {
            Some(rest)
        } else if digits > 0 && line[digits..].starts_with(". ") {
            Some(&line[digits + 1..])
        } else {
            None
        };
        match rest {
            Some(rest) => line = rest.trim_start(),
            None => return line,
        }
    }
}

/// `lines` (the runs' text cut, wrapped or padded) as runs again, each char in the role of the
/// char it came from; a char with none (`…`, padding) is plain.
pub fn restyle(lines: &[String], runs: &[Run]) -> Vec<Vec<Run>> {
    let mut from = runs
        .iter()
        .flat_map(|(text, strong)| text.chars().map(move |c| (c, *strong)))
        .peekable();
    lines
        .iter()
        .map(|line| {
            let mut out = Vec::new();
            for c in line.chars() {
                if !c.is_whitespace() {
                    while from.next_if(|(s, _)| s.is_whitespace()).is_some() {}
                }
                let strong = from
                    .next_if(|(s, _)| *s == c || s.is_whitespace() && c.is_whitespace())
                    .is_some_and(|(_, strong)| strong);
                push(&mut out, &c.to_string(), strong);
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    /// Runs drawn as text, strong ones in `[…]`.
    fn drawn(runs: &[Run]) -> String {
        runs.iter()
            .map(|(text, strong)| {
                if *strong {
                    format!("[{text}]")
                } else {
                    text.clone()
                }
            })
            .collect()
    }

    #[test]
    fn inline_markdown_maps_to_the_strong_role_and_loses_its_markers() {
        for (text, want) in [
            ("**Tests pass**: 12 passed", "[Tests pass]: 12 passed"),
            ("__done__ now", "[done] now"),
            ("ran `cargo test` twice", "ran [cargo test] twice"),
            ("``a ` b`` c", "[a ` b] c"),
            ("see [the PR](https://x.test/pr/31)", "see the PR"),
            ("[run `make`](https://x.test)", "run [make]"),
            (
                "**bold with `code` and [a link](u)**",
                "[bold with code and a link]",
            ),
            ("**unclosed bold", "**unclosed bold"),
            ("a `stray tick", "a `stray tick"),
            ("[not a link] (x)", "[not a link] (x)"),
            ("** spaced **", "** spaced **"),
            (
                "snake_case and __init__.py and a__b__c",
                "snake_case and [init].py and a__b__c",
            ),
            (
                "2*3*4 and *italic* and _italic_",
                "2*3*4 and *italic* and _italic_",
            ),
            (
                "https://x.test/a__b__c?q=1*2",
                "https://x.test/a__b__c?q=1*2",
            ),
            ("修复 **解析器** 🙂", "修复 [解析器] 🙂"),
        ] {
            assert_eq!(drawn(&inline(text)), want, "{text}");
        }
    }

    #[test]
    fn a_line_loses_the_block_markers_it_leads_with() {
        for (line, want) in [
            ("## Summary", "Summary"),
            ("- item", "item"),
            ("* item", "item"),
            ("> - quoted item", "quoted item"),
            ("12. step", "step"),
            ("```rust", "rust"),
            ("#123 fixed", "#123 fixed"),
            ("-1 degrees", "-1 degrees"),
            ("3.5 shipped", "3.5 shipped"),
            ("*italic* stays", "*italic* stays"),
        ] {
            assert_eq!(unblock(line), want, "{line}");
        }
    }

    #[test]
    fn cut_or_wrapped_text_keeps_each_chars_role() {
        let runs = inline("ok **two words** and `修复` end");
        let cut = crate::output::fit("ok two words and 修复 end", 13);
        assert_eq!(cut.width(), 13);
        assert_eq!(drawn(&restyle(&[cut], &runs)[0]), "ok [two words]…");
        let wrapped = [
            "ok two".to_owned(),
            "words and 修复".to_owned(),
            "end".to_owned(),
        ];
        let lines: Vec<String> = restyle(&wrapped, &runs).iter().map(|l| drawn(l)).collect();
        assert_eq!(lines, ["ok [two]", "[words] and [修复]", "end"]);
    }
}

#[cfg(test)]
mod board_tests {
    use chrono::{DateTime, Utc};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    use crate::tui::app::testkit::*;
    use crate::tui::app::{App, Effect, Spot};

    const MSG: &str = "## **Tests pass**: ran `cargo test` on [the PR](https://x.test/31) \
                       ⏎ - next: **merge** it ⏎ \x1b[31mred\x1b[0m snake_case 修复 **解析器**";

    fn board() -> App {
        let mut app = app();
        let mut rows = rows();
        rows[1].pane.msg = Some(MSG.into());
        app.set_rows(rows, hosts());
        app.selected = Some(Spot::Row("local/claude".into()));
        app
    }

    /// Each line of the screen, with whether the cells of `word` on it are bold.
    fn screen(app: &App, width: u16) -> Vec<(String, Vec<(usize, bool)>)> {
        let now = DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal
            .draw(|frame| crate::tui::view::draw(frame, app, now.with_timezone(&Utc)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..30)
            .map(|y| {
                let cells: Vec<_> = (0..width).map(|x| &buffer[(x, y)]).collect();
                let text: String = cells.iter().map(|c| c.symbol()).collect();
                let bold = cells
                    .iter()
                    .enumerate()
                    .map(|(x, c)| (x, c.modifier.contains(Modifier::BOLD)))
                    .collect();
                (text.trim_end().to_owned(), bold)
            })
            .collect()
    }

    fn bold(line: &(String, Vec<(usize, bool)>), word: &str) -> bool {
        let at = line.0[..line.0.find(word).unwrap()].chars().count();
        (at..at + word.chars().count()).all(|x| line.1[x].1)
    }

    #[test]
    fn msg_and_the_card_show_inline_markdown_and_the_copy_stays_raw() {
        let mut app = board();
        let lines = screen(&app, 200);
        let row = lines.iter().find(|l| l.0.contains("✓")).unwrap();
        assert!(
            row.0.ends_with("Tests pass: ran cargo test on the PR"),
            "MSG drops the heading and the markers: {}",
            row.0
        );
        assert!(bold(row, "Tests pass") && bold(row, "cargo test") && !bold(row, "ran"));
        let card: Vec<_> = lines
            .iter()
            .skip_while(|l| !l.0.starts_with("━━"))
            .collect();
        let at = |text: &str| *card.iter().find(|l| l.0.contains(text)).unwrap();
        assert_eq!(
            at("Tests pass").0,
            " ## Tests pass: ran cargo test on the PR"
        );
        assert_eq!(
            at("next").0,
            "   - next: merge it",
            "the card keeps block markers"
        );
        assert!(bold(at("next"), "merge") && !bold(at("next"), "next"));
        // A wide character's second cell reads as a space in the test buffer.
        assert_eq!(
            at("snake_case").0,
            "   red snake_case 修 复  解 析 器",
            "the escape is gone before mapping"
        );
        let line = at("snake_case");
        assert!(bold(line, "解") && bold(line, "器") && !bold(line, "修") && !bold(line, "snake"));
        assert!(matches!(
            typed(&mut app, "yy").pop().flatten(),
            Some(Effect::Copy { text, .. })
                if text == crate::output::clean(MSG) && text.starts_with("## **Tests pass**: ran `")
        ));
    }

    #[test]
    fn msg_is_cut_by_what_shows_not_by_its_markers() {
        let lines = screen(&board(), 80);
        let row = lines.iter().find(|l| l.0.contains("✓")).unwrap();
        assert!(
            row.0.ends_with("Tests pass: ran cargo test on …"),
            "{}",
            row.0
        );
        assert!(bold(row, "Tests pass") && !bold(row, "…"));
    }

    #[test]
    fn a_report_is_mapped_once_and_again_when_it_changes() {
        let mut app = board();
        let row = app.rows[1].clone();
        let first = app.marked(&row);
        assert!(std::rc::Rc::ptr_eq(&first, &app.marked(&row)), "kept");
        let mut rows = app.rows.clone();
        rows[1].pane.updated_at = Some("2026-01-01T00:01:00Z".into());
        rows[1].pane.msg = Some("**new**".into());
        app.set_rows(rows, hosts());
        let second = app.marked(&app.rows[1].clone());
        assert_eq!(second.msg, [("new".to_owned(), true)]);
    }
}
