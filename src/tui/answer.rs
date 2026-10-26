//! Answer mode (`a`): one row's pane, read fresh, answered with a digit or a line of text while
//! the board's own keys are off. Bound to the row's key, so a re-sort cannot move it.

use std::cell::Cell;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use unicode_width::UnicodeWidthStr;

use super::card::GUTTER;
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
        Some(Expect {
            updated_at: self.updated_at.clone(),
            fingerprint: self.screen.as_ref()?.fingerprint.clone()?,
            title: (self.row.pane.source == "bridge").then(|| self.row.pane.title.clone()),
        })
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
            self.error = Some(format!("there is no option {digit}"));
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
        let why = if !screen.options.is_empty() {
            "pick a number; this dialog takes no text"
        } else if screen.fingerprint.is_none() {
            "no input line on the screen; nothing sent"
        } else if screen.draft {
            "the input line already has text, which a reply would join; nothing sent"
        } else {
            return Step::Send(Action::Text(text));
        };
        self.error = Some(why.to_owned());
        Step::Stay
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
                    .map(|line| Line::styled(format!(" {line}"), Style::new().fg(Color::Red))),
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
        let right = match (self.captured, &self.screen) {
            (Some(at), _) => format!(
                " captured {} · {} ━━",
                at.with_timezone(&Local).format("%H:%M:%S"),
                age(&at.to_rfc3339(), now)
            ),
            (None, None) => " capturing… ━━".to_owned(),
            (None, Some(_)) => "━━".to_owned(),
        };
        let place = path(&[row.pane.session.as_str(), row.pane.tab.as_str()], 24);
        let mut parts = vec![name(row), place, row.pane.host.clone(), harness(row)];
        let room = width.saturating_sub("━━".width() + right.width() + 1);
        let text = |parts: &[String]| clean(&format!(" ANSWER → {} ", parts.join(" · ")));
        // Harness, then host, give way before the place is cut.
        while parts.len() > 2 && text(&parts).width() > room {
            parts.pop();
        }
        let left = fit(&text(&parts), room);
        let left = left.trim_end().to_owned() + " ";
        let fill = width.saturating_sub("━━".width() + left.width() + right.width());
        let red = Style::new().fg(Color::Red);
        Line::from(vec![
            Span::styled("━━", red),
            Span::styled(left, red.add_modifier(Modifier::BOLD)),
            Span::styled("━".repeat(fill), red),
            Span::styled(right, red),
        ])
    }

    /// The newest `room` rows of the screen, ending at a dialog's last option, with each option
    /// bright and a lasting one marked.
    fn screen_lines(&self, screen: &Screen, width: usize, room: usize) -> Vec<Line<'static>> {
        let end = screen
            .options
            .last()
            .map_or(screen.lines.len(), |c| c.line + 1);
        let text = width.saturating_sub(GUTTER.width()).max(1);
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
                theme::bright()
            } else {
                theme::dim()
            };
            for (at, piece) in pieces.into_iter().enumerate().rev() {
                let line = Line::from(vec![
                    Span::styled(GUTTER, theme::dim()),
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
            return Line::styled(fit(&format!(" sending {what}…"), width), theme::accent());
        }
        if self.screen.is_none() {
            return Line::styled(fit(" reading the pane…", width), theme::dim());
        }
        if let Some(choice) = self.armed_now(now) {
            let text = format!(" press {} again to send it: {}", choice.digit, choice.label);
            return Line::styled(fit(&text, width), theme::accent());
        }
        // A dialog takes no text, so the reply line shows no cursor to invite it.
        if self.screen.as_ref().is_some_and(|s| !s.options.is_empty()) {
            return Line::styled(" ›", theme::dim());
        }
        Line::styled(
            fit(&format!(" › {}▏", self.input.value()), width),
            theme::bright(),
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
            return Line::styled(fit(&keys, width), theme::bright());
        }
        Line::from(vec![
            Span::styled(keys, theme::bright()),
            Span::raw(" ".repeat(gap)),
            Span::styled(CHECKED, theme::dim()),
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
    use crate::tui::app::tests::row;

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
}
