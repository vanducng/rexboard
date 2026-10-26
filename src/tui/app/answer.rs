//! Answer mode on the board: opening it behind the typing guard, its keys, the read it waits for,
//! and what a send leaves behind.

use super::*;

impl App {
    /// `a` or `i`: answer mode on the current row, once a fresh read of its pane comes back.
    /// Pressed in the middle of typing, it opens nothing.
    pub(super) fn start_answer(&mut self, now: Instant) -> Option<Effect> {
        if !self.quiet {
            self.say(TOO_FAST_TO_ANSWER);
            return None;
        }
        let Some(row) = self.current() else {
            self.say("select a row to answer it");
            return None;
        };
        if self.is_own(row) {
            self.say("that is the board's own pane");
            return None;
        }
        let Some(updated_at) = row.pane.updated_at.clone() else {
            self.say("this pane reports no status, so it cannot be answered here · enter jumps");
            return None;
        };
        let row = row.clone();
        self.full = None;
        self.keys = false;
        let effect = read(&row);
        self.answer = Some(Answer::new(row, updated_at));
        self.answer_guard = Some(now);
        Some(effect)
    }

    /// A key in answer mode. A first key within GUARD of the opening means the opener was
    /// typing meant for somewhere else: answer mode leaves at once, and stays left, so no later
    /// pause lets the rest of the typing pick or fill the reply.
    pub(super) fn answer_key(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        let leaving = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if let Some(at) = self.answer_guard.take()
            && !leaving
            && now.duration_since(at) < GUARD
        {
            self.answer = None;
            self.say(TOO_FAST_TO_ANSWER);
            return None;
        }
        let answer = self.answer.as_mut()?;
        match answer.key(key, now) {
            Step::Stay => None,
            Step::Leave => {
                self.answer = None;
                None
            }
            Step::Next => {
                self.answer = None;
                self.next_waiting(now, false)
            }
            Step::Send(action) => {
                let expect = answer.expect()?;
                let what = answer.describe(&action);
                answer.sending = Some(what.clone());
                Some(Effect::Send {
                    key: answer.row.key.clone(),
                    pane: Box::new(answer.row.pane.clone()),
                    expect,
                    action,
                    what,
                })
            }
        }
    }

    /// A read for answer mode came back.
    pub fn screen(&mut self, key: &str, result: Result<Screen, String>) {
        let Some(answer) = self.answer.as_mut().filter(|a| a.row.key == key) else {
            return;
        };
        match result {
            Ok(screen) => {
                // Text typed for an input line must never meet a dialog.
                if !screen.options.is_empty() {
                    answer.input.reset();
                }
                answer.shown.set(0);
                answer.screen = Some(screen);
                answer.captured = Some(chrono::Utc::now());
            }
            Err(why) => {
                answer.screen = Some(Screen::default());
                answer.error = Some(why);
            }
        }
    }

    /// A send finished. Sent: back to the board, with the card's bar saying so. Refused: stay,
    /// with the reason, and read the pane again, against the row's latest report.
    pub fn sent(&mut self, key: &str, what: &str, result: Result<(), String>) -> Option<Effect> {
        let latest = self.rows.iter().find(|row| row.key == key).cloned();
        let answering = self.answer.as_ref().is_some_and(|a| a.row.key == key);
        let why = match result {
            Ok(()) => {
                let report = match self.answer.take_if(|a| a.row.key == key) {
                    Some(answer) => Some(answer.updated_at),
                    None => latest.as_ref().and_then(|row| row.pane.updated_at.clone()),
                };
                // A tail from before the answer shows the question it answered, and one read now
                // could catch the dialog before the agent takes the key: wait for its next report.
                self.peek.take_if(|peek| peek.key == key);
                if let Some(row) = &latest {
                    self.tailed = Some((row.key.clone(), row.pane.updated_at.clone()));
                }
                let to = latest.as_ref().map_or_else(|| key.to_owned(), name);
                let at = chrono::Local::now().format("%H:%M:%S");
                self.sent = Some(Sent {
                    key: key.to_owned(),
                    text: format!("sent {what} → {to} {at}"),
                    report,
                });
                return None;
            }
            Err(why) if !answering => {
                self.say(why);
                return None;
            }
            Err(why) => why,
        };
        let answer = self.answer.as_mut()?;
        if let Some(row) = latest
            && let Some(at) = row.pane.updated_at.clone()
        {
            answer.updated_at = at;
            answer.row = row;
        }
        answer.sending = None;
        answer.input.reset();
        answer.armed = None;
        answer.screen = None;
        answer.captured = None;
        answer.error = Some(why);
        Some(read(&answer.row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    /// Typing meant for an agent that lands on the board: the burst hits `dd` and `a`, and
    /// nothing closes and nothing is sent.
    #[test]
    fn a_typed_burst_closes_nothing_and_sends_nothing() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut app = app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(0));
        app.handle_at(KeyEvent::from(KeyCode::Char('d')), at(40));
        assert!(app.close_question(usize::MAX).is_some(), "dd asks");
        assert!(
            app.handle_at(KeyEvent::from(KeyCode::Char('y')), at(80))
                .is_none(),
            "a y within GUARD of the question closes nothing"
        );
        assert_eq!(app.toast(), Some("nothing closed (keys came too fast)"));
        let mut app = super::testkit::app();
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        let effects = typed_at(&mut app, "added hidden\n", 60, None, IDLE);
        assert!(!effects.iter().any(acts_on_a_pane), "{effects:?}");
        assert!(app.answer.is_none(), "the burst left answer mode");
    }

    /// Typing meant for an agent, with one ordinary pause anywhere in it, sends nothing and
    /// picks nothing: an opener only opens on its own, and a fast key after it leaves for good.
    #[test]
    fn typing_with_one_pause_never_sends_or_picks() {
        for text in [
            "please run tests\n",
            "fix it and run tests\n",
            "hidden middle ok\n",
        ] {
            for pause in 0..text.chars().count() {
                let mut app = app();
                let effects = typed_at(&mut app, text, 120, Some(pause), IDLE);
                assert!(
                    !effects.iter().any(acts_on_a_pane),
                    "{text:?} paused at {pause}: {effects:?}"
                );
            }
        }
        for pause in 0.."please run 1 test".len() {
            let mut app = app();
            let effects = typed_at(&mut app, "please run 1 test", 120, Some(pause), DIALOG);
            assert!(
                !effects.iter().any(acts_on_a_pane),
                "paused at {pause}: {effects:?}"
            );
        }
    }

    /// A seeded spread of random typing, 20 to 280 ms a key with one 400 ms pause, at dialogs
    /// and idle prompts: nothing sends, picks or closes.
    #[test]
    fn random_typing_with_a_pause_never_acts_on_a_pane() {
        let mut seed: u64 = 0x5eed;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let letters: Vec<char> = "abcdefghijklmnopqrstuvwxyz123".chars().collect();
        for round in 0..20_000 {
            let mut text = String::new();
            for _ in 0..=next(12) {
                for _ in 0..=next(8) {
                    text.push(letters[next(letters.len() as u64) as usize]);
                }
                text.push(if next(6) == 0 { '\n' } else { ' ' });
            }
            let pause = next(text.chars().count() as u64) as usize;
            let screen = if round % 2 == 0 { IDLE } else { DIALOG };
            let mut app = app();
            let t0 = Instant::now();
            let mut ms = 0;
            for (i, c) in text.chars().enumerate() {
                ms += if i == pause { 400 } else { 20 + next(261) };
                let code = if c == '\n' {
                    KeyCode::Enter
                } else {
                    KeyCode::Char(c)
                };
                let effect = app.handle_at(KeyEvent::from(code), t0 + Duration::from_millis(ms));
                if let Some(Effect::Read { key, .. }) = &effect {
                    app.screen(key, Ok(Screen::parse(screen, None)));
                    drawn(&app);
                }
                // The one way through: the typing starts with `a`, `i` or `dd` and pauses
                // right after it, which is what a person pressing it does.
                let opener_then_pause = (text.starts_with(['a', 'i']) && pause == 1)
                    || (text.starts_with("dd") && pause == 2);
                assert!(
                    !effect.as_ref().is_some_and(acts_on_a_pane) || opener_then_pause,
                    "{text:?} paused at {pause}: {effect:?}"
                );
            }
        }
    }

    #[test]
    fn answer_mode_leaves_on_a_fast_key_and_an_opener_needs_a_quiet_gap() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut app = app();
        app.handle_at(KeyEvent::from(KeyCode::Char('i')), at(0));
        assert!(app.answer.is_some(), "i opens answer mode too");
        app.handle_at(KeyEvent::from(KeyCode::Char('x')), at(200));
        assert!(app.answer.is_none(), "a fast first key leaves");
        assert_eq!(
            app.toast(),
            Some("keys came too fast; nothing sent · a answers")
        );
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(400));
        assert!(
            app.answer.is_none(),
            "an opener right after a key opens nothing"
        );
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(1_000));
        assert!(app.answer.is_some(), "on its own it opens");
        app.handle_at(KeyEvent::from(KeyCode::Esc), at(1_100));
        assert!(app.answer.is_none(), "esc always leaves");
        app.handle_at(KeyEvent::from(KeyCode::Char('a')), at(2_000));
        app.handle_at(KeyEvent::from(KeyCode::Char('z')), at(2_400));
        assert!(
            app.answer.is_some(),
            "a first key after GUARD is the answer"
        );
        assert_eq!(
            app.answer.as_ref().unwrap().input.value(),
            "",
            "before the read, it waits"
        );
    }

    #[test]
    fn answer_a_reads_the_pane_then_a_digit_sends_to_the_row_it_was_opened_on() {
        let mut app = app();
        assert_eq!(
            press(&mut app, KeyCode::Char('a')),
            Some(Effect::Read {
                key: "mini/seat-b".into(),
                host: "mini".into(),
                session: "session:api".into(),
                block: "seat-b".into(),
            })
        );
        assert_eq!(
            press(&mut app, KeyCode::Char('1')),
            None,
            "waits for the read"
        );
        app.screen("mini/seat-b", Ok(Screen::parse(DIALOG, None)));
        drawn(&app);
        let mut moved = rows();
        moved.reverse();
        app.set_rows(moved, hosts());
        let Some(Effect::Send {
            key,
            pane,
            expect,
            action,
            what,
        }) = press(&mut app, KeyCode::Char('1'))
        else {
            panic!("1 sends");
        };
        assert_eq!((key.as_str(), pane.host.as_str()), ("mini/seat-b", "mini"));
        assert_eq!(action, Action::Pick('1'));
        assert_eq!(what, "1 (Yes)");
        assert_eq!(
            (expect.state.as_str(), expect.kind.as_deref()),
            ("blocked", None)
        );
        assert!(expect.fingerprint.starts_with("\n ❯ 1. Yes"));
        assert_eq!(expect.title, None, "a native row has no title check");
    }

    #[test]
    fn answer_board_keys_are_off_and_ctrl_c_leaves_without_quitting() {
        let mut app = app();
        answering(&mut app, "────\n❯\u{a0}\n");
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('s'),
            KeyCode::Char('1'),
            KeyCode::Char('f'),
            KeyCode::Char(':'),
            KeyCode::Char('q'),
        ] {
            assert_eq!(press(&mut app, code), None, "{code:?}");
        }
        assert_eq!(selected(&app), "mini/seat-b");
        assert!(!app.filtering && app.pending.is_none() && !app.commanding);
        assert_eq!(app.answer.as_ref().unwrap().input.value(), "js1f:q");
        press(&mut app, KeyCode::Esc);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle(ctrl_c), None, "not Quit");
        assert!(app.answer.is_none());
        assert_eq!(
            app.handle(ctrl_c),
            Some(Effect::Quit),
            "on the board it quits"
        );
    }

    #[test]
    fn answer_tab_leaves_for_the_next_row_that_needs_you() {
        let mut app = app();
        answering(&mut app, DIALOG);
        assert_eq!(press(&mut app, KeyCode::Tab), None);
        assert!(app.answer.is_none());
        assert_eq!(selected(&app), "local/claude");
    }

    #[test]
    fn answer_is_refused_on_the_boards_own_pane_and_a_pane_with_no_record() {
        let mut app = App::default();
        let mut quiet = row("local/plain", "idle", "needs_you", false);
        quiet.pane.updated_at = None;
        app.set_rows(
            vec![row("local/board", "idle", "needs_you", false), quiet],
            hosts(),
        );
        app.own = Some("board".into());
        assert_eq!(press(&mut app, KeyCode::Char('a')), None);
        assert_eq!(app.toast(), Some("that is the board's own pane"));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(press(&mut app, KeyCode::Char('a')), None);
        assert!(
            app.toast()
                .unwrap()
                .starts_with("this pane reports no status")
        );
        assert!(app.answer.is_none());
    }

    #[test]
    fn answer_a_refusal_keeps_the_mode_and_reads_again_against_the_newest_report() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        let mut newer = rows();
        newer[0].pane.updated_at = Some("2026-01-01T00:00:09Z".into());
        app.set_rows(newer, hosts());
        let again = app.sent(
            &key,
            "1 (Yes)",
            Err("the agent's status changed since it was read; nothing sent".into()),
        );
        assert!(matches!(again, Some(Effect::Read { .. })), "{again:?}");
        let answer = app.answer.as_ref().expect("still answering");
        assert_eq!(
            answer.error.as_deref(),
            Some("the agent's status changed since it was read; nothing sent")
        );
        assert_eq!(answer.updated_at, "2026-01-01T00:00:09Z");
        assert!(answer.screen.is_none() && answer.sending.is_none());
    }

    #[test]
    fn answer_text_typed_for_an_idle_prompt_never_reaches_a_dialog_that_replaced_it() {
        let mut app = app();
        let key = answering(&mut app, "────\n❯\u{a0}\n");
        for c in "go ahead".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Send {
                action: Action::Text(_),
                ..
            })
        ));
        let why = "the agent's status changed since it was read; nothing sent";
        assert!(matches!(
            app.sent(&key, "\"go ahead\"", Err(why.into())),
            Some(Effect::Read { .. })
        ));
        assert_eq!(
            app.answer.as_ref().unwrap().input.value(),
            "",
            "a refusal clears it"
        );
        app.screen(&key, Ok(Screen::parse(DIALOG, None)));
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        for c in "go ahead".chars() {
            assert_eq!(press(&mut app, KeyCode::Char(c)), None);
        }
        assert_eq!(
            app.answer.as_ref().unwrap().error.as_deref(),
            Some("pick a number; this dialog takes no text")
        );
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            None,
            "no text into a dialog"
        );
    }

    #[test]
    fn answer_the_sent_line_lasts_until_the_row_needs_you_again() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        app.sent(&key, "1 (Yes)", Ok(()));
        let report = |app: &mut App, state: &str, group: &str, at: &str| {
            let mut all = rows();
            all[0] = row("mini/seat-b", state, group, false);
            all[0].pane.updated_at = Some(at.into());
            app.set_rows(all, hosts());
        };
        report(&mut app, "blocked", "needs_you", "2026-01-01T00:00:00Z");
        assert!(app.sent.is_some(), "the same report: kept");
        report(&mut app, "working", "working", "2026-01-01T00:00:05Z");
        assert!(
            app.sent.is_some(),
            "kept while the agent works on the answer"
        );
        report(&mut app, "blocked", "needs_you", "2026-01-01T00:01:00Z");
        assert_eq!(app.sent, None, "a new ask drops it");
    }

    #[test]
    fn answer_sent_returns_to_the_board_on_the_same_row_and_says_what_went() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.sent(&key, "1 (Yes)", Ok(())), None);
        assert!(app.answer.is_none());
        assert_eq!(selected(&app), "mini/seat-b");
        let sent = app.sent.clone().unwrap();
        assert_eq!(sent.key, "mini/seat-b");
        assert_eq!(sent.report.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert!(
            sent.text.starts_with("sent 1 (Yes) → seat-b "),
            "{}",
            sent.text
        );
    }

    #[test]
    fn answer_a_send_holds_the_auto_tail_until_the_next_report() {
        let mut app = app();
        let key = answering(&mut app, DIALOG);
        press(&mut app, KeyCode::Char('1'));
        // A hook's second report for the same dialog lands while the answer goes out.
        let mut rows = rows();
        rows[0].pane.updated_at = Some("2026-01-01T00:00:06Z".into());
        app.set_rows(rows.clone(), hosts());
        app.sent(&key, "1 (Yes)", Ok(()));
        let t0 = Instant::now();
        app.note_card(t0);
        assert_eq!(
            app.auto_tail(t0 + Duration::from_secs(1)),
            None,
            "not the answered report"
        );
        rows[0].pane.updated_at = Some("2026-01-01T00:01:00Z".into());
        app.set_rows(rows, hosts());
        assert!(
            matches!(
                app.auto_tail(t0 + Duration::from_secs(2)),
                Some(Effect::Peek { .. })
            ),
            "a new ask is read"
        );
    }
}
