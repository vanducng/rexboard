//! Answer mode (`a`): one row's pane, read fresh, answered with a digit or a line of text while
//! the board's own keys are off. Bound to the row's key, so a re-sort cannot move it.

use std::cell::Cell;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use unicode_width::UnicodeWidthStr;

use super::screen::{Choice, Screen};
use super::theme;
use crate::board::Row;
use crate::output::{age, chop, clean, fit, harness, name, path};
use crate::send::{Action, Expect};

/// A lasting option's second press must come within this.
pub const ARM: Duration = Duration::from_secs(3);
const CHECKED: &str = "the pane is re-checked before sending · board keys are off";

pub struct Answer {
    /// The row as it was when `a` was pressed.
    pub row: Row,
    /// None while the pane is being read.
    pub screen: Option<Screen>,
    pub captured: Option<DateTime<Utc>>,
    /// The row's report when the screen was asked for; the send requires it unchanged.
    pub updated_at: String,
    pub input: Input,
    /// A lasting option pressed once, and when.
    pub armed: Option<(char, Instant)>,
    /// Why the last key or send did nothing, in red.
    pub error: Option<String>,
    /// What is on its way to the pane.
    pub sending: Option<String>,
    /// The options whose line the card drew last, one bit per digit; a digit picks only these.
    pub shown: Cell<u16>,
}

/// What a key in answer mode leads to.
#[derive(Debug, PartialEq)]
pub enum Step {
    Stay,
    Leave,
    /// Leave, then `tab` on the board.
    Next,
    Send(Action),
}

/// What `row`'s pane must still show, as `screen` read it, for a key to go out; None with no
/// dialog or input line to check against.
pub fn expect(row: &Row, screen: &Screen) -> Option<Expect> {
    Some(Expect {
        state: row.pane.state.clone(),
        kind: row.pane.kind.clone(),
        fingerprint: screen.fingerprint.clone()?,
        title: (row.pane.source == "bridge").then(|| row.pane.title.clone()),
    })
}

/// Why a line of text cannot go to `screen`, or None when it can.
pub fn text_refusal(screen: &Screen) -> Option<&'static str> {
    if !screen.options.is_empty() {
        Some("pick a number; this dialog takes no text")
    } else if screen.fingerprint.is_none() {
        Some("no input line on the screen; nothing sent")
    } else if screen.draft {
        Some("the input line already has text, which a reply would join; nothing sent")
    } else {
        None
    }
}

pub fn no_option(digit: char) -> String {
    format!("there is no option {digit}")
}

/// A short id for what `screen` shows, so an answer can require the screen its caller read: a
/// dialog by its lines alone, which hold through a second report of the same dialog; an input
/// line with `report`, the row's `updated_at`, since the same empty prompt comes back every
/// turn. FNV-1a, so it holds across builds.
pub fn dialog_id(screen: &Screen, report: Option<&str>) -> Option<String> {
    let mut text = screen.fingerprint.clone()?;
    if screen.options.is_empty() {
        text.push('\n');
        text.push_str(report.unwrap_or_default());
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    Some(format!("{:012x}", hash >> 16))
}

/// The dialog's options as `show --json` and a refusal list them.
pub fn options_json(screen: &Screen) -> serde_json::Value {
    serde_json::Value::Array(
        screen
            .options
            .iter()
            .map(|c| {
                serde_json::json!({
                    "digit": c.digit.to_digit(10),
                    "label": c.label,
                    "lasting": c.lasting(),
                })
            })
            .collect(),
    )
}

impl Answer {
    pub fn new(row: Row, updated_at: String) -> Self {
        Self {
            row,
            screen: None,
            captured: None,
            updated_at,
            input: Input::default(),
            armed: None,
            error: None,
            sending: None,
            shown: Cell::new(0),
        }
    }

    /// What the pane must still show for `action` to go out.
    pub fn expect(&self) -> Option<Expect> {
        expect(&self.row, self.screen.as_ref()?)
    }

    /// `1 (Yes)` or `"fix the test"`, as the card's bar reports a send.
    pub fn describe(&self, action: &Action) -> String {
        match action {
            Action::Pick(digit) => {
                let label = self.choice(*digit).map_or("", |c| c.label.as_str());
                format!("{digit} ({})", fit(label, 24).trim_end())
            }
            Action::Text(text) => format!("\"{}\"", fit(text, 24).trim_end()),
        }
    }

    /// The options on screen, in order.
    fn visible(&self) -> Vec<char> {
        let options = self.screen.as_ref().map_or(&[][..], |s| &s.options[..]);
        options
            .iter()
            .map(|c| c.digit)
            .filter(|d| self.shown.get() & bit(*d) != 0)
            .collect()
    }

    /// Whether the card drew every option.
    pub fn all_shown(&self) -> bool {
        self.visible().len() == self.screen.as_ref().map_or(0, |s| s.options.len())
    }

    fn choice(&self, digit: char) -> Option<&Choice> {
        self.screen
            .as_ref()?
            .options
            .iter()
            .find(|c| c.digit == digit)
    }

    pub fn key(&mut self, key: KeyEvent, now: Instant) -> Step {
        let chord = !(key.modifiers - KeyModifiers::SHIFT).is_empty();
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::Leave;
        }
        match key.code {
            KeyCode::Esc if !self.input.value().is_empty() => {
                self.input.reset();
                self.error = None;
                return Step::Stay;
            }
            KeyCode::Esc => return Step::Leave,
            KeyCode::Tab => return Step::Next,
            _ => {}
        }
        let Some(screen) = &self.screen else {
            return Step::Stay;
        };
        if chord || self.sending.is_some() {
            return Step::Stay;
        }
        let dialog = !screen.options.is_empty();
        self.error = None;
        match key.code {
            KeyCode::Char(c @ '1'..='9') if dialog => self.pick(c, now),
            KeyCode::Char(_) if dialog => {
                self.error = Some("pick a number; this dialog takes no text".to_owned());
                Step::Stay
            }
            KeyCode::Enter => self.reply(),
            _ => {
                self.input.handle_event(&Event::Key(key));
                Step::Stay
            }
        }
    }

    /// A digit sends its option; a lasting one only on its second press within ARM.
    fn pick(&mut self, digit: char, now: Instant) -> Step {
        let Some(choice) = self.choice(digit) else {
            self.error = Some(no_option(digit));
            return Step::Stay;
        };
        if self.shown.get() & bit(digit) == 0 {
            self.error = Some(format!(
                "option {digit} is off screen; make the board taller or jump to it"
            ));
            return Step::Stay;
        }
        let again = self
            .armed
            .is_some_and(|(armed, at)| armed == digit && now.duration_since(at) < ARM);
        if choice.lasting() && !again {
            self.armed = Some((digit, now));
            return Step::Stay;
        }
        self.armed = None;
        Step::Send(Action::Pick(digit))
    }

    fn reply(&mut self) -> Step {
        let text = clean(self.input.value()).trim().to_owned();
        let Some(screen) = self.screen.as_ref().filter(|_| !text.is_empty()) else {
            return Step::Stay;
        };
        match text_refusal(screen) {
            Some(why) => {
                self.error = Some(why.to_owned());
                Step::Stay
            }
            None => Step::Send(Action::Text(text)),
        }
    }

    fn armed_now(&self, now: Instant) -> Option<&Choice> {
        let (digit, at) = self.armed?;
        (now.duration_since(at) < ARM)
            .then(|| self.choice(digit))
            .flatten()
    }

    /// The card in answer mode: the red bar, the pane's last lines down to the dialog's last
    /// option, then the reply line and what the next key does.
    pub fn lines(&self, now: DateTime<Utc>, width: usize, room: usize) -> Vec<Line<'static>> {
        let mut lines = vec![self.bar(now, width)];
        let mut below = Vec::new();
        if let Some(why) = &self.error {
            below.extend(
                chop(why, width.saturating_sub(1).max(1))
                    .into_iter()
                    .map(|line| Line::styled(format!(" {line}"), theme::alert())),
            );
        }
        below.push(self.reply_line(width));
        let room = room.saturating_sub(lines.len() + below.len());
        if let Some(screen) = &self.screen {
            lines.extend(self.screen_lines(screen, width, room));
        }
        lines.extend(below);
        lines
    }

    /// `━━ ANSWER → seat-b · web › deploy · local · claude ━━━ captured 11:22:01 · 1s ━━`
    fn bar(&self, now: DateTime<Utc>, width: usize) -> Line<'static> {
        let row = &self.row;
        let ends = theme::BAR_END;
        let right = match (self.captured, &self.screen) {
            (Some(at), _) => format!(
                " captured {} · {} {ends}",
                at.with_timezone(&Local).format("%H:%M:%S"),
                age(&at.to_rfc3339(), now)
            ),
            (None, None) => format!(" capturing… {ends}"),
            (None, Some(_)) => ends.to_owned(),
        };
        let place = path(&[row.pane.session.as_str(), row.pane.tab.as_str()], 24);
        let mut parts = vec![name(row), place, row.pane.host.clone(), harness(row)];
        let room = width.saturating_sub(ends.width() + right.width() + 1);
        let text = |parts: &[String]| clean(&format!(" ANSWER → {} ", parts.join(" · ")));
        // Harness, then host, give way before the place is cut.
        while parts.len() > 2 && text(&parts).width() > room {
            parts.pop();
        }
        let left = fit(&text(&parts), room);
        let left = left.trim_end().to_owned() + " ";
        let fill = width.saturating_sub(ends.width() + left.width() + right.width());
        Line::from(vec![
            Span::styled(ends, theme::alert()),
            Span::styled(left, theme::alert_label()),
            Span::styled(theme::RULE.repeat(fill), theme::alert()),
            Span::styled(right, theme::alert()),
        ])
    }

    /// The newest `room` rows of the screen, ending at a dialog's last option, with each option
    /// bright and a lasting one marked.
    fn screen_lines(&self, screen: &Screen, width: usize, room: usize) -> Vec<Line<'static>> {
        let end = screen
            .options
            .last()
            .map_or(screen.lines.len(), |c| c.line + 1);
        let text = width.saturating_sub(theme::GUTTER.width()).max(1);
        // Each row, and the digit when it is an option's first row.
        let mut rows: Vec<(Line<'static>, Option<char>)> = Vec::new();
        for (i, line) in screen.lines[..end].iter().enumerate().rev() {
            if rows.len() >= room {
                break;
            }
            let choice = screen.options.iter().find(|c| c.line == i);
            let mark = choice
                .filter(|c| c.lasting())
                .map(|c| format!("   press {} twice", c.digit))
                .unwrap_or_default();
            // The mark is never cut: the label wraps to make room for it, under itself.
            let indent = choice.map_or_else(
                || line.width() - line.trim_start().width(),
                |c| line.find(&c.label).map_or(0, |at| line[..at].width()),
            );
            let mut pieces = if line.chars().all(|c| matches!(c, '─' | '━' | '╌' | ' ')) {
                vec![line.chars().take(text).collect()]
            } else {
                hang(line, text.saturating_sub(mark.width()).max(1), indent)
            };
            if let Some(first) = pieces.first_mut() {
                first.push_str(&mark);
            }
            let style = if choice.is_some() {
                theme::strong()
            } else {
                theme::quiet()
            };
            for (at, piece) in pieces.into_iter().enumerate().rev() {
                let line = Line::from(vec![
                    Span::styled(theme::GUTTER, theme::quiet()),
                    Span::styled(piece, style),
                ]);
                rows.push((line, choice.filter(|_| at == 0).map(|c| c.digit)));
            }
        }
        rows.truncate(room);
        self.shown.set(
            rows.iter()
                .filter_map(|(_, digit)| *digit)
                .fold(0, |bits, digit| bits | bit(digit)),
        );
        rows.into_iter().rev().map(|(line, _)| line).collect()
    }

    /// ` › fix the test▏`, or what is happening instead.
    fn reply_line(&self, width: usize) -> Line<'static> {
        let now = Instant::now();
        if let Some(what) = &self.sending {
            return Line::styled(fit(&format!(" sending {what}…"), width), theme::note());
        }
        if self.screen.is_none() {
            return Line::styled(fit(" reading the pane…", width), theme::quiet());
        }
        if let Some(choice) = self.armed_now(now) {
            let text = format!(" press {} again to send it: {}", choice.digit, choice.label);
            return Line::styled(fit(&text, width), theme::accent());
        }
        // A dialog takes no text, so the reply line shows no cursor to invite it.
        if self.screen.as_ref().is_some_and(|s| !s.options.is_empty()) {
            return Line::styled(format!(" {}", theme::PROMPT), theme::quiet());
        }
        Line::styled(
            fit(
                &format!(" {} {}{}", theme::PROMPT, self.input.value(), theme::CURSOR),
                width,
            ),
            theme::strong(),
        )
    }

    /// The footer: what keys do here, and the promise that nothing goes out unchecked.
    pub fn footer(&self, width: usize) -> Line<'static> {
        let options = self.screen.as_ref().map_or(0, |s| s.options.len());
        let visible = self.visible();
        let esc = if self.input.value().is_empty() {
            "esc back to board"
        } else {
            "esc clears"
        };
        let keys = match &self.screen {
            None => esc.to_owned(),
            Some(_) if options > 0 => match (visible.first(), visible.last()) {
                (Some(a), Some(b)) if a == b => format!("{a} pick · {esc}"),
                (Some(a), Some(b)) => format!("{a}-{b} pick · {esc}"),
                _ => format!("options are off screen · {esc}"),
            },
            Some(screen) if screen.fingerprint.is_none() => {
                format!("{esc} · no dialog or input line on this screen")
            }
            Some(screen) if screen.draft => {
                format!("{esc} · the input line has text, so jump to it")
            }
            Some(_) => format!("type to reply · enter send · {esc}"),
        };
        let gap = width.saturating_sub(keys.width() + CHECKED.width());
        if gap < 4 {
            return Line::styled(fit(&keys, width), theme::strong());
        }
        Line::from(vec![
            Span::styled(keys, theme::strong()),
            Span::raw(" ".repeat(gap)),
            Span::styled(CHECKED, theme::quiet()),
        ])
    }
}

fn bit(digit: char) -> u16 {
    1 << digit.to_digit(10).unwrap_or(0)
}

/// `line` cut into rows of `width`, the rows after the first indented by `indent`.
fn hang(line: &str, width: usize, indent: usize) -> Vec<String> {
    let mut pieces = chop(line, width);
    if pieces.len() < 2 || indent * 2 > width {
        return pieces;
    }
    let rest = pieces.split_off(1).concat();
    let pad = " ".repeat(indent);
    pieces.extend(
        chop(&rest, width - indent)
            .into_iter()
            .map(|piece| format!("{pad}{piece}")),
    );
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::Peek;
    use crate::tui::app::testkit::*;

    fn dialog() -> Answer {
        let mut answer = Answer::new(
            row("mini/seat-b", "blocked", "needs_you", false),
            "2026-01-01T00:00:00Z".into(),
        );
        let text = "Bash wants to run:\n  git push\n ❯ 1. Yes\n   2. Yes, and don't ask again for git push commands\n   3. No\n Esc to cancel\n";
        answer.screen = Some(Screen::parse(text, None));
        answer.lines(Utc::now(), 120, 40);
        answer
    }

    fn idle() -> Answer {
        let mut answer = dialog();
        answer.screen = Some(Screen::parse("⏺ done\n────\n❯\u{a0}\n────\n", None));
        answer
    }

    fn key(answer: &mut Answer, code: KeyCode, now: Instant) -> Step {
        answer.key(KeyEvent::from(code), now)
    }

    #[test]
    fn answer_pick_is_one_digit_and_a_missing_option_says_so() {
        let mut answer = dialog();
        let now = Instant::now();
        assert_eq!(
            key(&mut answer, KeyCode::Char('1'), now),
            Step::Send(Action::Pick('1'))
        );
        assert_eq!(key(&mut answer, KeyCode::Char('7'), now), Step::Stay);
        assert_eq!(answer.error.as_deref(), Some("there is no option 7"));
        assert_eq!(answer.describe(&Action::Pick('1')), "1 (Yes)");
    }

    #[test]
    fn answer_persistent_needs_a_second_press_within_3s() {
        let mut answer = dialog();
        let t0 = Instant::now();
        assert_eq!(key(&mut answer, KeyCode::Char('2'), t0), Step::Stay);
        let hint = answer.reply_line(120).to_string();
        assert!(hint.contains("press 2 again to send it"), "{hint}");
        assert_eq!(
            key(&mut answer, KeyCode::Char('2'), t0 + Duration::from_secs(1)),
            Step::Send(Action::Pick('2'))
        );
        assert_eq!(key(&mut answer, KeyCode::Char('2'), t0), Step::Stay);
        assert_eq!(
            key(&mut answer, KeyCode::Char('2'), t0 + ARM),
            Step::Stay,
            "too late: it arms again"
        );
        assert_eq!(
            key(
                &mut answer,
                KeyCode::Char('2'),
                t0 + ARM + Duration::from_secs(1)
            ),
            Step::Send(Action::Pick('2'))
        );
    }

    #[test]
    fn answer_text_is_refused_by_a_dialog_and_sent_at_an_idle_prompt() {
        let now = Instant::now();
        let mut answer = dialog();
        assert_eq!(key(&mut answer, KeyCode::Char('f'), now), Step::Stay);
        assert_eq!(
            answer.error.as_deref(),
            Some("pick a number; this dialog takes no text")
        );
        let mut answer = idle();
        assert_eq!(key(&mut answer, KeyCode::Enter, now), Step::Stay, "empty");
        for c in "fix the test".chars() {
            assert_eq!(key(&mut answer, KeyCode::Char(c), now), Step::Stay);
        }
        assert_eq!(
            key(&mut answer, KeyCode::Enter, now),
            Step::Send(Action::Text("fix the test".into()))
        );
    }

    #[test]
    fn answer_enter_never_sends_held_text_into_a_dialog() {
        let mut answer = dialog();
        answer.input = Input::new("go ahead".into());
        let now = Instant::now();
        assert_eq!(key(&mut answer, KeyCode::Enter, now), Step::Stay);
        assert_eq!(
            answer.error.as_deref(),
            Some("pick a number; this dialog takes no text")
        );
        assert_eq!(
            answer.reply_line(80).to_string(),
            " ›",
            "no cursor in a dialog"
        );
    }

    #[test]
    fn answer_text_is_refused_over_a_draft() {
        let mut answer = idle();
        answer.screen = Some(Screen::parse("────\n❯ half typed\n", None));
        let now = Instant::now();
        key(&mut answer, KeyCode::Char('x'), now);
        assert_eq!(key(&mut answer, KeyCode::Enter, now), Step::Stay);
        assert!(answer.error.as_deref().unwrap().ends_with("nothing sent"));
    }

    #[test]
    fn answer_esc_clears_then_leaves_and_ctrl_c_and_tab_leave_without_a_send() {
        let now = Instant::now();
        let mut answer = idle();
        key(&mut answer, KeyCode::Char('x'), now);
        assert_eq!(key(&mut answer, KeyCode::Esc, now), Step::Stay);
        assert_eq!(answer.input.value(), "");
        assert_eq!(key(&mut answer, KeyCode::Esc, now), Step::Leave);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(dialog().key(ctrl_c, now), Step::Leave);
        assert_eq!(key(&mut dialog(), KeyCode::Tab, now), Step::Next);
    }

    #[test]
    fn answer_keys_wait_while_the_pane_is_read() {
        let mut answer = dialog();
        answer.screen = None;
        let now = Instant::now();
        assert_eq!(key(&mut answer, KeyCode::Char('1'), now), Step::Stay);
        assert_eq!(key(&mut answer, KeyCode::Enter, now), Step::Stay);
        assert_eq!(key(&mut answer, KeyCode::Esc, now), Step::Leave);
    }

    #[test]
    fn answer_mode_shows_a_red_bar_the_options_and_its_own_footer_at_80_and_140() {
        let app = answer_mode_on(PERMISSION);
        for width in [80, 140] {
            let text = render(&app, width, 24);
            let card = card_of(&text);
            assert!(
                card[0].starts_with("━━ ANSWER → seat-b · api › main · mini · claude "),
                "{text}"
            );
            assert!(card[0].contains("captured"), "{text}");
            assert!(card.iter().any(|l| l == " │  ❯ 1. Yes"), "{text}");
            assert!(
                card.iter()
                    .any(|l| l.contains("2. Yes, and don't ask") && l.ends_with("press 2 twice")),
                "{text}"
            );
            assert!(
                !text.contains("Esc to cancel"),
                "chrome below the options is cut: {text}"
            );
            assert_eq!(
                card.last().unwrap(),
                " ›",
                "no cursor under a dialog: {text}"
            );
            let footer = text.lines().last().unwrap();
            assert!(
                footer.starts_with("1-3 pick · esc back to board"),
                "{footer}"
            );
            assert_eq!(
                footer.contains("board keys are off"),
                width == 140,
                "the promise shows when it fits: {footer}"
            );
        }
    }

    #[test]
    fn answer_mode_says_why_a_key_did_nothing_and_shows_a_pending_double_press() {
        let mut app = answer_mode_on(PERMISSION);
        press(&mut app, KeyCode::Char('x'));
        let text = render(&app, 100, 24);
        assert!(
            text.contains(" pick a number; this dialog takes no text"),
            "{text}"
        );
        press(&mut app, KeyCode::Char('2'));
        let text = render(&app, 100, 24);
        assert!(
            text.contains(" press 2 again to send it: Yes, and don't ask again"),
            "{text}"
        );
    }

    #[test]
    fn after_a_send_the_card_drops_the_tail_from_before_the_answer() {
        let mut app = answer_mode_on(PERMISSION);
        app.peek = Some(Peek {
            key: "mini/seat-b".into(),
            lines: Some(Ok(PERMISSION.lines().map(str::to_owned).collect())),
            at: Some(Utc::now()),
            updated_at: None,
        });
        press(&mut app, KeyCode::Char('1'));
        app.sent("mini/seat-b", "1 (Yes)", Ok(()));
        assert!(app.peek.is_none());
        let card = card_of(&render(&app, 140, 24)).join("\n");
        assert!(!card.contains("Do you want to proceed"), "{card}");
        assert!(card.contains(" sent 1 (Yes) → seat-b "), "{card}");
    }

    #[test]
    fn after_a_send_the_bar_says_what_went_where() {
        let mut app = answer_mode_on(PERMISSION);
        press(&mut app, KeyCode::Char('1'));
        app.sent("mini/seat-b", "1 (Yes)", Ok(()));
        let text = render(&app, 140, 24);
        let bar = &card_of(&text)[0];
        assert!(bar.contains(" sent 1 (Yes) → seat-b "), "{bar}");
    }

    #[test]
    fn answer_a_short_board_shows_every_option_or_refuses_the_hidden_ones() {
        let four = include_str!("../../tests/fixtures/screens/claude-permission.txt");
        for text in [PERMISSION, four] {
            for (height, refused) in [(12, false), (9, false), (13, true), (6, false)] {
                let mut app = answer_mode_on(text);
                if refused {
                    app.answer.as_mut().unwrap().error =
                        Some("the screen changed since it was read; nothing sent".into());
                }
                let screen = render(&app, 80, height);
                let footer = screen.lines().last().unwrap().to_owned();
                let digits: Vec<char> = app
                    .answer
                    .as_ref()
                    .unwrap()
                    .screen
                    .as_ref()
                    .unwrap()
                    .options
                    .iter()
                    .map(|c| c.digit)
                    .collect();
                let picks = footer
                    .split(" pick")
                    .next()
                    .filter(|_| footer.contains(" pick"));
                let shown: Vec<char> = match picks.map(|p| p.split_once('-')) {
                    Some(Some((a, b))) => {
                        let (a, b) = (a.chars().next().unwrap(), b.chars().next().unwrap());
                        digits
                            .iter()
                            .copied()
                            .filter(|d| (a..=b).contains(d))
                            .collect()
                    }
                    Some(None) => picks.unwrap().chars().take(1).collect(),
                    None => Vec::new(),
                };
                if height >= 12 {
                    assert_eq!(shown, digits, "every option fits at 80x{height}: {screen}");
                }
                for digit in digits {
                    let drawn = screen.lines().any(|l| l.contains(&format!("{digit}. ")));
                    let mut copy = answer_mode_on(text);
                    render(&copy, 80, height);
                    let sent = press(&mut copy, KeyCode::Char(digit));
                    if shown.contains(&digit) {
                        assert!(drawn, "{digit} is in the footer, so on the card: {screen}");
                    } else {
                        assert_eq!(sent, None, "{digit} is off screen at 80x{height}");
                        let why = copy.answer.as_ref().unwrap().error.clone().unwrap();
                        assert_eq!(
                            why,
                            format!(
                                "option {digit} is off screen; make the board taller or jump to it"
                            )
                        );
                    }
                }
            }
        }
    }
}
