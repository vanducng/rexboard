//! A pane's lines with their styles. Rex's `vt` format keeps SGR colours and writes runs of
//! spaces as cursor-forward (`ESC[nC`); [`sanitize`] keeps only the SGR sequences, and [`spans`]
//! turns them into ratatui styles.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// Cursor-forward never moves further than a line could be wide.
const MOST_FORWARD: usize = 1024;

/// A vt line with only its SGR sequences left: `ESC[nC` becomes n spaces, a tab one, and every
/// other escape sequence and control character goes, as the pane's programs wrote them.
pub fn sanitize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => csi(&mut chars, &mut out),
                Some(']' | 'P' | '_' | '^') => skip_string(&mut chars),
                // `ESC ( B` and the like: intermediate bytes, then one final byte.
                Some('\x20'..='\x2f') => {
                    while chars.next_if(|c| ('\x20'..='\x2f').contains(c)).is_some() {}
                    chars.next();
                }
                _ => {}
            },
            '\u{9b}' => csi(&mut chars, &mut out),
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One CSI sequence after its `ESC[`: an SGR is kept, a cursor-forward becomes spaces, and
/// anything else, or one cut short, is dropped.
fn csi(chars: &mut std::iter::Peekable<std::str::Chars>, out: &mut String) {
    let mut params = String::new();
    for c in chars.by_ref() {
        match c {
            '0'..='9' | ';' | ':' => params.push(c),
            'm' => {
                out.push_str("\x1b[");
                out.push_str(&params);
                out.push('m');
                return;
            }
            'C' => {
                let n = params.parse::<usize>().unwrap_or(1).clamp(1, MOST_FORWARD);
                out.extend(std::iter::repeat_n(' ', n));
                return;
            }
            '\x20'..='\x2f' | '<'..='?' => params.push('?'),
            _ => return,
        }
    }
}

/// An OSC, DCS, APC or PM string, up to its BEL or `ESC \`.
fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(c) = chars.next() {
        if c == '\x07' || c == '\x1b' && chars.next_if_eq(&'\\').is_some() {
            return;
        }
    }
}

/// A sanitized line as styled spans, each SGR applied over the ones before it on the line.
pub fn spans(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut style = Style::new();
    let mut text = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("\x1b[") {
        text.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find('m') else {
            rest = "";
            break;
        };
        if !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style));
        }
        style = sgr(style, &after[..end]);
        rest = &after[end + 1..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    spans
}

/// `style` after one SGR's parameters: reset, the modifiers on and off, and the 16, 256 and
/// RGB colours in both `;` and `:` forms. Anything else, or out of range, changes nothing.
fn sgr(mut style: Style, params: &str) -> Style {
    let mut codes = params.split(';');
    while let Some(code) = codes.next() {
        if code.contains(':') {
            let parts: Vec<&str> = code.split(':').collect();
            match parts[0] {
                "38" => style.fg = colour(&parts[1..], true).or(style.fg),
                "48" => style.bg = colour(&parts[1..], true).or(style.bg),
                "4" if parts.get(1) == Some(&"0") => {
                    style = style.remove_modifier(Modifier::UNDERLINED);
                }
                "4" => style = style.add_modifier(Modifier::UNDERLINED),
                _ => {}
            }
            continue;
        }
        let Ok(n) = code
            .parse::<u16>()
            .or(if code.is_empty() { Ok(0) } else { Err(()) })
        else {
            continue;
        };
        style = match n {
            0 => Style::new(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            7 => style.add_modifier(Modifier::REVERSED),
            9 => style.add_modifier(Modifier::CROSSED_OUT),
            22 => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            27 => style.remove_modifier(Modifier::REVERSED),
            29 => style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style.fg(Color::Indexed((n - 30) as u8)),
            90..=97 => style.fg(Color::Indexed((n - 90 + 8) as u8)),
            40..=47 => style.bg(Color::Indexed((n - 40) as u8)),
            100..=107 => style.bg(Color::Indexed((n - 100 + 8) as u8)),
            39 => Style { fg: None, ..style },
            49 => Style { bg: None, ..style },
            38 | 48 | 58 => {
                let rest: Vec<&str> = codes.clone().collect();
                let taken = match rest.first() {
                    Some(&"5") => 2,
                    Some(&"2") => 4,
                    _ => 0,
                };
                for _ in 0..taken.min(rest.len()) {
                    codes.next();
                }
                match (n, colour(&rest, false)) {
                    (38, Some(c)) => style.fg(c),
                    (48, Some(c)) => style.bg(c),
                    _ => style,
                }
            }
            _ => style,
        };
    }
    style
}

/// A colour from the parameters after 38 or 48: `5;n` or `2;r;g;b`. The `:` form may carry an
/// empty colour-space id before the RGB.
fn colour(parts: &[&str], colon: bool) -> Option<Color> {
    let byte = |s: &&str| s.parse::<u8>().ok();
    match parts {
        ["5", n, ..] => byte(n).map(Color::Indexed),
        ["2", "", r, g, b, ..] if colon => Some(Color::Rgb(byte(r)?, byte(g)?, byte(b)?)),
        ["2", r, g, b, ..] => Some(Color::Rgb(byte(r)?, byte(g)?, byte(b)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(line: &str) -> Vec<(String, Style)> {
        spans(&sanitize(line))
            .into_iter()
            .map(|s| (s.content.into_owned(), s.style))
            .collect()
    }

    /// Rex's vt for `printf '\e[31mone\ntwo\e[0m\nthree\n'`: it restates the style at each
    /// line's start and resets it at the end, so every line stands alone.
    #[test]
    fn a_colour_across_lines_comes_back_on_each_line_from_rex() {
        let vt = "\x1b[0m\x1b[38;5;1mone\x1b[0m\r\n\x1b[0m\x1b[38;5;1mtwo\x1b[0m\r\nthree\r\n";
        let lines: Vec<Vec<(String, Style)>> = vt.lines().map(styles).collect();
        let red = Style::new().fg(Color::Indexed(1));
        assert_eq!(lines[0], [("one".to_owned(), red)]);
        assert_eq!(
            lines[1],
            [("two".to_owned(), red)],
            "the second line is red on its own"
        );
        assert_eq!(lines[2], [("three".to_owned(), Style::new())]);
    }

    #[test]
    fn cursor_forward_is_spaces_and_other_escapes_go() {
        assert_eq!(
            sanitize("\x1b[1CQuick\x1b[1Csafety\x1b[3Ccheck\x1b[C!\r"),
            " Quick safety   check !"
        );
        assert_eq!(
            sanitize("a\x1b]0;title\x07b\x1b]8;;http://x\x1b\\c\x1b[2Jd\x1b[?25le\x1b(Bf\tg\x07"),
            "abcdef g"
        );
        assert_eq!(sanitize("\x1b[99999C").len(), MOST_FORWARD, "capped");
        assert_eq!(
            sanitize("cut \x1b[38;2;1"),
            "cut ",
            "a sequence cut short goes"
        );
        assert_eq!(sanitize("x\x1b[1;31my"), "x\x1b[1;31my", "SGR stays");
    }

    #[test]
    fn each_sgr_kind_styles_the_text_after_it() {
        let got = styles("\x1b[31mred\x1b[1mbold\x1b[22;39mplain");
        assert_eq!(got[0], ("red".into(), Style::new().fg(Color::Indexed(1))));
        assert_eq!(
            got[1],
            (
                "bold".into(),
                Style::new()
                    .fg(Color::Indexed(1))
                    .add_modifier(Modifier::BOLD)
            )
        );
        assert_eq!(got[2].0, "plain");
        assert_eq!(got[2].1.fg, None);
        assert!(!got[2].1.add_modifier.contains(Modifier::BOLD));
        let cases = [
            ("\x1b[92mx", Style::new().fg(Color::Indexed(10))),
            ("\x1b[44mx", Style::new().bg(Color::Indexed(4))),
            ("\x1b[103mx", Style::new().bg(Color::Indexed(11))),
            ("\x1b[38;5;208mx", Style::new().fg(Color::Indexed(208))),
            (
                "\x1b[48;2;10;20;30mx",
                Style::new().bg(Color::Rgb(10, 20, 30)),
            ),
            ("\x1b[38:2::1:2:3mx", Style::new().fg(Color::Rgb(1, 2, 3))),
            ("\x1b[38:2:1:2:3mx", Style::new().fg(Color::Rgb(1, 2, 3))),
            ("\x1b[38:5:9mx", Style::new().fg(Color::Indexed(9))),
            ("\x1b[2mx", Style::new().add_modifier(Modifier::DIM)),
            ("\x1b[3mx", Style::new().add_modifier(Modifier::ITALIC)),
            ("\x1b[4mx", Style::new().add_modifier(Modifier::UNDERLINED)),
            (
                "\x1b[4:3mx",
                Style::new().add_modifier(Modifier::UNDERLINED),
            ),
            ("\x1b[7mx", Style::new().add_modifier(Modifier::REVERSED)),
        ];
        for (line, want) in cases {
            assert_eq!(styles(line), [("x".to_owned(), want)], "{line:?}");
        }
    }

    #[test]
    fn resets_clear_what_they_name_and_an_empty_sgr_clears_all() {
        let all = "\x1b[1;2;3;4;7;31;42m";
        for (off, gone) in [
            ("0", Style::new()),
            ("", Style::new()),
            ("22;23;24;27;39;49", Style::new()),
        ] {
            let got = styles(&format!("{all}a\x1b[{off}mb"));
            assert_eq!(got[1].0, "b");
            assert_eq!(got[1].1.fg, gone.fg, "{off:?}");
            assert_eq!(got[1].1.bg, gone.bg, "{off:?}");
            assert!(got[1].1.add_modifier.is_empty(), "{off:?}: {:?}", got[1].1);
        }
    }

    #[test]
    fn a_malformed_sgr_changes_nothing_it_cannot_read() {
        for line in [
            "\x1b[38;5mx",
            "\x1b[38;2;1;2mx",
            "\x1b[38;2;300;0;0mx",
            "\x1b[38;9;1mx",
            "\x1b[999mx",
            "\x1b[;5;mx",
            "\x1b[58;5;3mx",
        ] {
            let got = styles(line);
            assert_eq!(got.last().unwrap().0, "x", "{line:?}: {got:?}");
            assert_eq!(got.last().unwrap().1.fg, None, "{line:?}");
        }
        assert_eq!(
            styles("\x1b[58;5;3;31mx"),
            [("x".to_owned(), Style::new().fg(Color::Indexed(1)))],
            "the underline colour's arguments are skipped, and what follows applies"
        );
    }
}
