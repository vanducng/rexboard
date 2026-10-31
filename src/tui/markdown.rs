//! The inline markdown an agent's message carries, as runs of text in the plain or strong role:
//! `**bold**`, `__bold__` and `` `code` `` are strong, `[text](url)` is its text, and the markers
//! go. Italics stay as written, so `snake_case` and `2*3*4` read as they are.

use unicode_width::UnicodeWidthStr;

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
            && let Some(paren) = closing_paren(&chars, close + 2)
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

/// The `)` that closes a link's URL from `from` on, past any balanced pair inside it
/// (`https://w.test/Foo_(bar)`).
fn closing_paren(chars: &[char], from: usize) -> Option<usize> {
    let mut depth = 0;
    (from..chars.len()).find(|&j| match chars[j] {
        '(' => {
            depth += 1;
            false
        }
        ')' if depth > 0 => {
            depth -= 1;
            false
        }
        c => c == ')',
    })
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

/// What MSG shows of a row's message: its line as runs, or none for a hook's filler (`Done`,
/// `Allow Bash?`). The board draws it and sorts by it.
pub fn msg(row: &crate::board::Row) -> Vec<Run> {
    if crate::output::turn_msg(row).is_empty() {
        return Vec::new();
    }
    let msg = crate::output::clean(row.pane.msg.as_deref().unwrap_or_default());
    msg_line(&lines(&msg))
}

/// A cleaned message's lines, as the hook kept them: blank ones and leading spaces included.
pub fn lines(msg: &str) -> Vec<&str> {
    msg.split(crate::output::BREAK).map(str::trim_end).collect()
}

/// The text of `runs`, as it shows.
pub fn text(runs: &[Run]) -> String {
    runs.iter().map(|(text, _)| text.as_str()).collect()
}

/// MSG's line among the message's: the first with words that is not a heading, else the first
/// heading. A fence line is never one.
pub fn msg_line(lines: &[&str]) -> Vec<Run> {
    let worded = |line: &&&str| !fence(line) && !unblock(line).is_empty();
    let line = lines
        .iter()
        .filter(worded)
        .find(|line| heading(line).is_none())
        .or_else(|| lines.iter().find(worded));
    line.map_or_else(Vec::new, |line| inline(unblock(line)))
}

/// How the card lays out one line of a message.
#[derive(Debug, PartialEq)]
pub enum Block {
    /// Wraps; a heading's text is all strong.
    Text(Vec<Run>),
    /// A list item: wraps under its text, the marker (`- `, `12. `) on the first row only.
    Item(String, Vec<Run>),
    /// A quote: wraps with `> ` on every row.
    Quote(Vec<Run>),
    /// A code line between fences, as typed, or a table row: cut at the edge, never wrapped.
    Cut(Vec<Run>, bool),
    /// The one blank line between paragraphs.
    Blank,
}

/// A message's lines as the card lays them out. Fence lines go and what is between them is code;
/// a table's separator row goes; runs of blank lines are one, and none at the start or end.
pub fn card(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut code = false;
    let mut at = 0;
    while let Some(line) = lines.get(at) {
        at += 1;
        let block = if fence(line) {
            code = !code;
            continue;
        } else if code {
            Block::Cut(vec![((*line).to_owned(), false)], true)
        } else if line.trim().is_empty() {
            Block::Blank
        } else if let Some(text) = heading(line) {
            Block::Text(strong(inline(text)))
        } else if row(line) {
            let start = at - 1;
            while lines.get(at).is_some_and(|line| row(line)) {
                at += 1;
            }
            blocks.extend(table(&lines[start..at]));
            continue;
        } else if let Some(rest) = line.trim_start().strip_prefix('>') {
            Block::Quote(inline(rest.trim_start()))
        } else if let Some((marker, rest)) = item(line) {
            Block::Item(marker.to_owned(), inline(rest))
        } else {
            Block::Text(inline(line.trim_start()))
        };
        let blank = block == Block::Blank;
        if !blank || blocks.last().is_some_and(|last| *last != Block::Blank) {
            blocks.push(block);
        }
    }
    if blocks.last() == Some(&Block::Blank) {
        blocks.pop();
    }
    align_numbers(&mut blocks);
    blocks
}

fn strong(runs: Vec<Run>) -> Vec<Run> {
    let mut out = Vec::new();
    for (text, _) in runs {
        push(&mut out, &text, true);
    }
    out
}

/// A table row: a line that starts with `|`.
fn row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A table's rows with their columns padded to each one's widest cell, as it shows; the row
/// above the `|---|` separator, which goes, is the header and strong.
fn table(rows: &[&str]) -> Vec<Block> {
    let separator = |row: &str| row.trim().chars().all(|c| "|-: ".contains(c));
    let cells: Vec<(Vec<Vec<Run>>, bool)> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| !separator(row))
        .map(|(n, row)| {
            let header = rows.get(n + 1).is_some_and(|next| separator(next));
            let inner = row.trim();
            let inner = inner.strip_prefix('|').unwrap_or(inner);
            let inner = inner
                .strip_suffix('|')
                .filter(|rest| !rest.ends_with('\\'))
                .unwrap_or(inner);
            let cells = split_cells(inner)
                .into_iter()
                .map(|cell| inline(cell.trim()));
            (
                cells.map(|c| if header { strong(c) } else { c }).collect(),
                header,
            )
        })
        .collect();
    let mut widths: Vec<usize> = Vec::new();
    for (row, _) in &cells {
        for (n, cell) in row.iter().enumerate() {
            let width = text(cell).width();
            match widths.get_mut(n) {
                Some(widest) => *widest = (*widest).max(width),
                None => widths.push(width),
            }
        }
    }
    cells
        .into_iter()
        .map(|(row, _)| {
            let mut runs = vec![("|".to_owned(), false)];
            for (cell, widest) in row.iter().zip(&widths) {
                push(&mut runs, " ", false);
                for (text, strong) in cell {
                    push(&mut runs, text, *strong);
                }
                let pad = widest - text(cell).width();
                push(&mut runs, &format!("{} |", " ".repeat(pad)), false);
            }
            Block::Cut(runs, false)
        })
        .collect()
}

/// A table row's cells: split at each `|` but an escaped `\|`, which shows as `|`.
fn split_cells(row: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        let cell = cells.last_mut().expect("one cell");
        match c {
            '\\' if chars.peek() == Some(&'|') => cell.push(chars.next().expect("a pipe")),
            '|' => cells.push(String::new()),
            c => cell.push(c),
        }
    }
    cells
}

/// A numbered list's numbers right-aligned past 9 (` 9.`, `10.`), so its text keeps one edge.
fn align_numbers(blocks: &mut [Block]) {
    let number = |marker: &str| {
        let bare = marker.trim_start();
        let digits = bare.len() - bare.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        (digits > 0).then_some((marker.len() - bare.len(), digits))
    };
    for list in blocks.split_mut(|block| !matches!(block, Block::Item(..))) {
        let mut widest = std::collections::BTreeMap::new();
        for block in list.iter() {
            if let Block::Item(marker, _) = block
                && let Some((indent, digits)) = number(marker)
            {
                let most = widest.entry(indent).or_insert(0);
                *most = digits.max(*most);
            }
        }
        for block in list.iter_mut() {
            if let Block::Item(marker, _) = block
                && let Some((indent, digits)) = number(marker)
            {
                let pad = " ".repeat(widest[&indent] - digits);
                marker.insert_str(indent, &pad);
            }
        }
    }
}

/// A list item's marker, with its indent and its space (`- `, `  12. `), and its text.
fn item(line: &str) -> Option<(&str, &str)> {
    let bare = line.trim_start();
    let indent = line.len() - bare.len();
    let digits = bare.len() - bare.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let width = if ["- ", "* ", "+ "].iter().any(|m| bare.starts_with(m)) {
        2
    } else if digits > 0 && bare[digits..].starts_with(". ") {
        digits + 2
    } else {
        return None;
    };
    Some(line.split_at(indent + width))
}

/// A heading's text: what follows its `#`s and a space.
fn heading(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let hashes = line.len() - line.trim_start_matches('#').len();
    (hashes > 0 && line[hashes..].starts_with(' ')).then(|| line[hashes..].trim_start())
}

/// A code fence's line: three backticks and an info string, not code that closes on the line.
fn fence(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("```")
        .is_some_and(|rest| !rest.contains("```"))
}

/// A line without the block markers it leads with: a heading's `#`s, a quote's `>`, and a list's
/// `-`, `*`, `+` or `1.`.
pub fn unblock(line: &str) -> &str {
    let mut line = line.trim_start();
    loop {
        let rest = heading(line)
            .or_else(|| line.strip_prefix('>'))
            .or_else(|| item(line).map(|(_, rest)| rest));
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
            ("[Foo](https://w.test/Foo_(bar)) after", "Foo after"),
            (
                "[open](https://w.test/a(b) tail",
                "[open](https://w.test/a(b) tail",
            ),
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
            ("```x``` stays", "```x``` stays"),
            ("#123 fixed", "#123 fixed"),
            ("-1 degrees", "-1 degrees"),
            ("3.5 shipped", "3.5 shipped"),
            ("*italic* stays", "*italic* stays"),
        ] {
            assert_eq!(unblock(line), want, "{line}");
        }
    }

    #[test]
    fn msg_takes_the_first_line_with_words_that_is_not_a_heading() {
        for (lines, want) in [
            (
                &["## Summary", "**Tests pass**: 12", "- more"][..],
                "[Tests pass]: 12",
            ),
            (&["# Title", "```rust", "> quoted **bit**"], "quoted [bit]"),
            (&["## Only a heading"], "Only a heading"),
            (&["```sh", "## Plan"], "Plan"),
            (&["```sh"], ""),
            (&["`a` first", "## later"], "[a] first"),
        ] {
            assert_eq!(drawn(&msg_line(lines)), want, "{lines:?}");
        }
    }

    #[test]
    fn the_card_reads_each_line_as_a_block() {
        let show = |block: &Block| match block {
            Block::Text(runs) => drawn(runs),
            Block::Item(marker, runs) => format!("item {marker:?} {}", drawn(runs)),
            Block::Quote(runs) => format!("quote {}", drawn(runs)),
            Block::Cut(runs, true) => format!("code {:?}", drawn(runs)),
            Block::Cut(runs, false) => format!("row {}", drawn(runs)),
            Block::Blank => "blank".into(),
        };
        let lines = [
            "",
            "## The **plan**",
            "",
            "",
            "- a **b**",
            "12. twelve",
            "> quoted **bit**",
            "#123 fixed",
            "```x``` is code",
            "```rust",
            "    let **x** = 1;",
            "",
            "```",
            "| a | **b** |",
            "|---|:-:|",
            "",
        ];
        let blocks: Vec<String> = card(&lines).iter().map(show).collect();
        assert_eq!(
            blocks,
            [
                "[The plan]",
                "blank",
                "item \"- \" a [b]",
                "item \"12. \" twelve",
                "quote quoted [bit]",
                "#123 fixed",
                "[x] is code",
                "code \"    let **x** = 1;\"",
                "code \"\"",
                "row | [a] | [b] |",
            ],
            "blank runs are one, none at either end; fences and the table separator go"
        );
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

    const MSG: &str = "## Summary ⏎ **Tests pass**: ran `cargo test` on [the PR](https://x.test/31) \
                       ⏎ - next: **merge** it ⏎ ```sh ⏎ make **all** ⏎ ``` ⏎ \x1b[31mred\x1b[0m snake_case 修复 **解析器**";

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
            "MSG takes the line under the heading, without markers: {}",
            row.0
        );
        assert!(bold(row, "Tests pass") && bold(row, "cargo test") && !bold(row, "ran"));
        let card: Vec<_> = lines
            .iter()
            .skip_while(|l| !l.0.starts_with("━━"))
            .collect();
        let at = |text: &str| *card.iter().find(|l| l.0.contains(text)).unwrap();
        assert_eq!(
            at("Summary").0,
            " Summary",
            "the heading is the headline, without #"
        );
        assert!(bold(at("Summary"), "Summary"));
        assert_eq!(
            at("Tests pass").0,
            "   Tests pass: ran cargo test on the PR"
        );
        assert!(
            !card
                .iter()
                .any(|l| l.0.contains("```") || l.0.trim() == "sh"),
            "no fence"
        );
        assert_eq!(
            at("make").0,
            "     make **all**",
            "code as typed, 2 in from the text"
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
                if text == crate::output::clean(MSG) && text.starts_with("## Summary ⏎ **Tests pass**: ran `")
        ));
    }

    #[test]
    fn msg_is_cut_by_what_shows_not_by_its_markers() {
        let lines = screen(&board(), 80);
        let row = lines.iter().find(|l| l.0.contains("✓")).unwrap();
        assert!(row.0.ends_with("Tests pass: ran cargo test…"), "{}", row.0);
        assert!(bold(row, "Tests pass") && !bold(row, "…"));
    }

    /// The keys of the rows the list shows, in order.
    fn listed(app: &App) -> Vec<String> {
        app.items()
            .into_iter()
            .filter_map(|item| match item {
                crate::tui::app::Item::Row(i) => Some(app.rows[i].key.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sorting_by_msg_orders_rows_by_what_msg_shows() {
        let mut app = App::default();
        let mut rows = Vec::new();
        for (key, msg) in [
            ("local/zebra", "## Summary ⏎ Zebra done"),
            ("local/zeta", "**Zeta** done"),
            ("local/alpha", "Alpha done"),
        ] {
            let mut row = row(key, "idle", "quiet", true);
            row.pane.msg = Some(msg.into());
            rows.push(row);
        }
        app.set_rows(rows, hosts());
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('v'));
        sort(&mut app, 'm');
        assert_eq!(
            listed(&app),
            ["local/alpha", "local/zebra", "local/zeta"],
            "raw, `## summary` and `**zeta**` would sort first"
        );
    }

    #[test]
    fn the_filter_matches_a_phrase_as_it_shows_and_as_it_was_sent() {
        for (typed, kept) in [
            ("cargo test on", true),
            ("on the pr", true),
            ("next: merge", true),
            ("`cargo test`", true),
            ("x.test/31", true),
            ("cargo test off", false),
        ] {
            let mut app = board();
            press(&mut app, ratatui::crossterm::event::KeyCode::Char('/'));
            for c in typed.chars() {
                press(&mut app, ratatui::crossterm::event::KeyCode::Char(c));
            }
            assert_eq!(listed(&app) == ["local/claude"], kept, "/{typed}");
        }
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
