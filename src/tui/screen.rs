//! What answer mode reads off a pane's tail: the numbered options of a dialog, or the input line,
//! and the literal text `send.lua` must still find on screen before it sends.

use crate::output::clean;

/// Only the end of the screen counts, so an old dialog in the scrollback never matches;
/// `send.lua` uses the same window.
pub const WINDOW: usize = 40;

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub digit: char,
    pub label: String,
    /// Its line in `Screen::lines`.
    pub line: usize,
}

impl Choice {
    /// An option that writes a rule or changes the mode for later prompts too.
    pub fn lasting(&self) -> bool {
        let label = self.label.to_lowercase().replace('’', "'");
        ["don't ask again", "always", "auto mode"]
            .iter()
            .any(|words| label.contains(words))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screen {
    /// The window's lines, without escape sequences or control characters.
    pub lines: Vec<String>,
    pub options: Vec<Choice>,
    /// The option block, or the input line and the line above it, exactly as captured and
    /// wrapped in newlines, so only whole lines match: `❯ ` must not match `❯ half typed`.
    pub fingerprint: Option<String>,
    /// The input line already holds typed text, which a reply would join.
    pub draft: bool,
}

impl Screen {
    /// `text` is the plain tail; `vt` the end of the same pane with its styles, when read.
    pub fn parse(text: &str, vt: Option<&str>) -> Self {
        let mut raw: Vec<&str> = text.split('\n').collect();
        while raw.last().is_some_and(|line| line.trim_ascii().is_empty()) {
            raw.pop();
        }
        let raw = &raw[raw.len().saturating_sub(WINDOW)..];
        let lines: Vec<String> = raw.iter().map(|l| clean(l).trim_end().to_owned()).collect();
        let input = lines.iter().rposition(|line| prompt(line).is_some());
        let options = choices(&lines).filter(|options| {
            let last = options.last().expect("two or more").line;
            input.is_none_or(|p| p < last)
        });
        if let Some(options) = options {
            let (first, last) = (options[0].line, options.last().expect("two or more").line);
            return Self {
                fingerprint: Some(whole(&raw[first..=last])),
                lines,
                options,
                draft: false,
            };
        }
        let Some(p) = input else {
            return Self {
                lines,
                ..Self::default()
            };
        };
        let typed = !prompt(&lines[p]).unwrap_or_default().is_empty();
        Self {
            fingerprint: Some(whole(&raw[p.saturating_sub(1)..=p])),
            draft: typed && vt.is_none_or(vt_typed),
            lines,
            options: Vec::new(),
        }
    }
}

fn whole(lines: &[&str]) -> String {
    format!("\n{}\n", lines.join("\n"))
}

/// A dialog's options: the last run numbered 1, 2, 3… with at least two, allowing other lines
/// (wrapped labels, descriptions, rules) between them.
fn choices(lines: &[String]) -> Option<Vec<Choice>> {
    let all: Vec<Choice> = lines
        .iter()
        .enumerate()
        .filter_map(|(line, text)| {
            let (digit, label) = option(text)?;
            Some(Choice {
                digit,
                label: label.to_owned(),
                line,
            })
        })
        .collect();
    let start = all.iter().rposition(|c| c.digit == '1')?;
    let mut run: Vec<Choice> = Vec::new();
    for choice in &all[start..] {
        let next = char::from_digit(run.len() as u32 + 1, 10);
        if Some(choice.digit) != next {
            break;
        }
        run.push(choice.clone());
    }
    (run.len() >= 2).then_some(run)
}

/// `❯ 1. Yes` or `  2) No`: the digit and its label.
fn option(line: &str) -> Option<(char, &str)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix(['❯', '>']).unwrap_or(rest).trim_start();
    let mut chars = rest.chars();
    let digit = chars.next().filter(|c| ('1'..='9').contains(c))?;
    let rest = chars.as_str().strip_prefix(['.', ')'])?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let label = rest.trim();
    (!label.is_empty()).then_some((digit, label))
}

/// An input line, `❯ text` or `│ > text │`: what follows the prompt.
fn prompt(line: &str) -> Option<&str> {
    if option(line).is_some() {
        return None;
    }
    let rest = line.trim_start();
    let rest = rest.strip_prefix('│').unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix(['❯', '›', '>'])?;
    Some(rest.trim().trim_end_matches('│').trim())
}

/// Whether the last input line in `vt` has text that is not dim: a harness draws its placeholder
/// and its suggested next prompt dim, so in plain text they look typed.
fn vt_typed(vt: &str) -> bool {
    vt.split('\n')
        .rev()
        .map(styled)
        .find(|chars| {
            let text: String = chars.iter().map(|&(c, _)| c).collect();
            prompt(&text).is_some()
        })
        .is_some_and(|chars| {
            let at = chars
                .iter()
                .position(|&(c, _)| matches!(c, '❯' | '›' | '>'))
                .unwrap_or(0);
            chars[at + 1..]
                .iter()
                .any(|&(c, dim)| !dim && !c.is_whitespace() && c != '│')
        })
}

/// A vt line's printed characters, each with whether it is dim (SGR 2).
fn styled(line: &str) -> Vec<(char, bool)> {
    let mut out = Vec::new();
    let mut dim = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.next() != Some('[') {
                    continue;
                }
                let mut params = String::new();
                let end = chars.by_ref().find(|c| {
                    if ('\x40'..='\x7e').contains(c) {
                        return true;
                    }
                    params.push(*c);
                    false
                });
                match end {
                    Some('m') => dim = sgr_dim(&params, dim),
                    Some('C') => {
                        let n = params.parse().unwrap_or(1);
                        out.extend(std::iter::repeat_n((' ', false), n));
                    }
                    _ => {}
                }
            }
            c if c.is_control() => {}
            c => out.push((c, dim)),
        }
    }
    out
}

/// Dim after an SGR: 2 sets it, 0 (or none) and 22 clear it; the colour forms 38/48/58 carry
/// their own numbers, which are skipped.
fn sgr_dim(params: &str, mut dim: bool) -> bool {
    let mut parts = params.split([';', ':']);
    while let Some(part) = parts.next() {
        match part {
            "" | "0" | "22" => dim = false,
            "2" => dim = true,
            "38" | "48" | "58" => match parts.next() {
                Some("5") => {
                    parts.next();
                }
                Some("2") => {
                    parts.nth(2);
                }
                _ => {}
            },
            _ => {}
        }
    }
    dim
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/screens/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    fn labels(screen: &Screen) -> Vec<(char, &str)> {
        screen
            .options
            .iter()
            .map(|c| (c.digit, c.label.as_str()))
            .collect()
    }

    #[test]
    fn answer_reads_a_permission_dialog_and_its_lasting_options() {
        let screen = Screen::parse(&fixture("claude-permission.txt"), None);
        assert_eq!(
            labels(&screen),
            [
                ('1', "Yes"),
                (
                    '2',
                    "Yes, and always allow access to /work/project from this project"
                ),
                (
                    '3',
                    "Yes, and switch to auto mode · auto mode handles these prompts for you"
                ),
                ('4', "No"),
            ]
        );
        let lasting: Vec<bool> = screen.options.iter().map(Choice::lasting).collect();
        assert_eq!(lasting, [false, true, true, false]);
        let fp = screen.fingerprint.unwrap();
        assert!(fp.starts_with("\n ❯ 1. Yes\n"), "{fp:?}");
        assert!(fp.ends_with("   4. No\n"), "{fp:?}");
        assert!(!screen.draft);
    }

    #[test]
    fn answer_keeps_a_wrapped_label_inside_the_fingerprint() {
        let text = fixture("claude-permission-wrapped.txt");
        let screen = Screen::parse(&text, None);
        assert_eq!(screen.options.len(), 4);
        let fp = screen.fingerprint.unwrap();
        assert!(fp.contains("at-wraps-onto-a-second-line"), "{fp:?}");
        assert!(
            text.contains(&fp),
            "the fingerprint is literal capture text"
        );
    }

    #[test]
    fn answer_the_fingerprint_is_whole_lines_so_a_later_draft_breaks_it() {
        let empty = Screen::parse("────\n❯\u{a0}\n────\n", None);
        let fp = empty.fingerprint.unwrap();
        assert_eq!(fp, "\n────\n❯\u{a0}\n");
        let window = |text: &str| format!("\n{text}\n");
        assert!(window("────\n❯\u{a0}\n────").contains(&fp));
        assert!(
            !window("────\n❯\u{a0}half typed\n────").contains(&fp),
            "send.lua pads the window the same way, so a draft typed since is caught"
        );
    }

    #[test]
    fn answer_reads_ask_user_question_across_its_rule() {
        let screen = Screen::parse(&fixture("claude-ask-user-question.txt"), None);
        let digits: String = screen.options.iter().map(|c| c.digit).collect();
        assert_eq!(digits, "12345");
        assert_eq!(screen.options[0].label, "Red");
        assert_eq!(screen.options[4].label, "Chat about this");
        assert!(screen.fingerprint.unwrap().contains("Type something."));
    }

    #[test]
    fn answer_an_idle_prompt_has_no_options_and_no_draft() {
        let screen = Screen::parse(&fixture("claude-idle.txt"), None);
        assert!(screen.options.is_empty());
        assert!(!screen.draft);
        let fp = screen.fingerprint.unwrap();
        assert!(fp.starts_with("\n─") && fp.ends_with("❯\u{a0}\n"), "{fp:?}");
    }

    #[test]
    fn answer_a_numbered_list_above_the_input_line_is_not_a_dialog() {
        let text = "⏺ Next:\n  1. Fix the test\n  2. Ship it\n────\n❯\u{a0}\n────\n";
        let screen = Screen::parse(text, None);
        assert!(screen.options.is_empty());
        assert_eq!(screen.fingerprint.as_deref(), Some("\n────\n❯\u{a0}\n"));
    }

    #[test]
    fn answer_a_draft_is_typed_text_but_a_dim_placeholder_is_not() {
        let draft = Screen::parse(&fixture("claude-draft.txt"), None);
        assert!(draft.draft);
        let typed = "────\r\n❯\u{a0}draft\x1b[1Cwords\r\n────";
        assert!(Screen::parse(&fixture("claude-draft.txt"), Some(typed)).draft);
        let placeholder = fixture("claude-idle-placeholder.txt");
        assert!(
            Screen::parse(&placeholder, None).draft,
            "no vt: assume typed"
        );
        let dim = "\x1b[0m────\r\n❯\u{a0}\x1b[0m\x1b[2mTry \"fix lint errors\"\x1b[0m\r\n";
        assert!(!Screen::parse(&placeholder, Some(dim)).draft);
        let suggestion = fixture("claude-idle-after-list.txt");
        let dim = "❯\u{a0}\x1b[2mRun the touch command again\x1b[0m";
        assert!(!Screen::parse(&suggestion, Some(dim)).draft);
    }

    #[test]
    fn answer_colour_numbers_do_not_read_as_dim() {
        let vt = "❯\u{a0}\x1b[38;2;153;2;2mtyped\x1b[0m";
        assert!(vt_typed(vt), "2 inside a colour is not SGR 2");
        assert!(!vt_typed("❯\u{a0}\x1b[1;2mhint"));
        assert!(vt_typed("❯\u{a0}\x1b[2mhint\x1b[22m typed"));
    }

    #[test]
    fn answer_a_screen_with_no_prompt_or_dialog_offers_nothing() {
        let screen = Screen::parse("$ cargo test\nok\n", None);
        assert_eq!(screen.fingerprint, None);
        assert!(screen.options.is_empty());
    }
}
