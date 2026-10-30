//! `x`, then an extension action's key: its input line, then its confirm, then the run.
//! Nothing runs before `y`, and `y` runs only what the confirm line showed.

use ratatui::text::{Line, Span};

use super::*;
use crate::extension::action::Plan;
use crate::tui::theme;

pub struct Doing {
    offered: Offered,
    /// The row the action was started on.
    key: String,
    stage: Stage,
}

enum Stage {
    Text(Input),
    Confirm {
        plan: Box<Plan>,
        text: String,
        asked: Instant,
    },
}

impl App {
    /// The keys and words `x` offers for the current row.
    pub fn action_pairs(&self) -> Vec<(String, String)> {
        let Some(row) = self.current() else {
            return Vec::new();
        };
        self.offered
            .iter()
            .filter(|o| o.applies(row))
            .map(|o| (o.action.key.clone(), o.action.title.to_lowercase()))
            .collect()
    }

    /// The key after `x`: the action it names starts, anything else says what `x` takes.
    pub(super) fn start_action(&mut self, k: K) -> Option<Effect> {
        if k.0 == KeyCode::Esc {
            return None;
        }
        let pairs = self.action_pairs();
        let row = self.current()?.clone();
        let found = k.char().and_then(|c| {
            self.offered
                .iter()
                .find(|o| o.action.key == c.to_string() && o.applies(&row))
        });
        let Some(offered) = found.cloned() else {
            let keys: Vec<String> = pairs.into_iter().map(|(key, _)| key).collect();
            self.say(if keys.is_empty() {
                "no action for this row".to_owned()
            } else {
                format!("x takes {}; esc cancels", keys.join(" "))
            });
            return None;
        };
        if self.busy() {
            return None;
        }
        self.doing = Some(Doing {
            offered,
            key: row.key,
            stage: Stage::Text(Input::default()),
        });
        None
    }

    /// A key while an action is open: typing in its input line, or the answer to its confirm.
    pub(super) fn doing_key(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        let mut doing = self.doing.take()?;
        let title = doing.offered.action.title.clone();
        let stage = std::mem::replace(&mut doing.stage, Stage::Text(Input::default()));
        match stage {
            Stage::Text(mut input) => match key.code {
                KeyCode::Esc => self.say(format!("{title} cancelled")),
                KeyCode::Enter => {
                    let text = input.value().to_owned();
                    match self.plan_for(&doing, &text) {
                        Ok(plan) => {
                            doing.stage = Stage::Confirm {
                                plan: Box::new(plan),
                                text,
                                asked: now,
                            };
                        }
                        Err(why) => {
                            self.failed = Some(why);
                            doing.stage = Stage::Text(input);
                        }
                    }
                    self.doing = Some(doing);
                }
                _ => {
                    input.handle_event(&Event::Key(key));
                    doing.stage = Stage::Text(input);
                    self.doing = Some(doing);
                }
            },
            Stage::Confirm { plan, text, asked } => {
                if now.duration_since(asked) < GUARD {
                    self.say(format!("{title} cancelled (keys came too fast)"));
                } else if K::of(key) != Some(K(KeyCode::Char('y'), false)) {
                    self.say(format!("{title} cancelled"));
                } else {
                    // The row moved or changed since the confirm was drawn: it is not the
                    // question that was answered.
                    match self.plan_for(&doing, &text) {
                        Ok(again) if again != *plan => {
                            self.failed = Some(format!("{title} cancelled: the target changed"));
                        }
                        Ok(_) if self.busy() => {}
                        Ok(again) => {
                            self.going = Some(format!("{title}…"));
                            return Some(Effect::Run {
                                plan: Box::new(again),
                                title,
                            });
                        }
                        Err(why) => self.failed = Some(format!("{title} cancelled: {why}")),
                    }
                }
            }
        }
        None
    }

    /// The plan for the row the action started on, as the board has it now.
    fn plan_for(&self, doing: &Doing, text: &str) -> Result<Plan, String> {
        let row = self
            .rows
            .iter()
            .find(|row| row.key == doing.key)
            .ok_or("that row is gone")?;
        doing.offered.plan(row, text)
    }

    /// The footer while an action waits: its input line, or its confirm.
    pub fn action_footer(&self, width: usize) -> Option<Line<'static>> {
        let doing = self.doing.as_ref()?;
        match &doing.stage {
            Stage::Text(input) => {
                let row = self.rows.iter().find(|row| row.key == doing.key)?;
                // The end being typed keeps at least half the line; the head gives way.
                let typed = ends(input.value(), width / 2);
                let room = width.saturating_sub(typed.width() + theme::CURSOR.width());
                let (title, host) = (&doing.offered.action.title, &row.pane.host);
                let around = title.width() + " ".width() + format!(" on {host}: ").width();
                let place = self.place(row, room.saturating_sub(around).max(1));
                let head = crate::output::squeeze(&format!("{title} {place} on {host}: "), room);
                let tail = " · enter · esc";
                let mut spans = vec![
                    Span::styled(head, theme::strong()),
                    Span::styled(format!("{typed}{}", theme::CURSOR), theme::strong()),
                ];
                if spans.iter().map(|s| s.content.width()).sum::<usize>() + tail.width() <= width {
                    spans.push(Span::styled(tail, theme::quiet()));
                }
                Some(Line::from(spans))
            }
            Stage::Confirm { plan, .. } => {
                let tail = " · y runs";
                let said =
                    crate::output::squeeze(&plan.confirm, width.saturating_sub(tail.width()));
                Some(Line::styled(format!("{said}{tail}"), theme::alert_label()))
            }
        }
    }
}

/// The last `room` columns of `text`, so the end being typed stays in view.
fn ends(text: &str, room: usize) -> String {
    let mut used = 0;
    let mut kept: Vec<char> = text
        .chars()
        .rev()
        .take_while(|c| {
            used += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
            used <= room
        })
        .collect();
    kept.reverse();
    kept.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    fn run_of(effect: Option<Effect>) -> Plan {
        match effect {
            Some(Effect::Run { plan, .. }) => *plan,
            other => panic!("expected a run, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn x_s_opens_the_input_and_nothing_runs_before_the_confirm_and_y() {
        let mut app = with_actions();
        assert!(typed(&mut app, "xs").iter().all(Option::is_none));
        assert!(app.doing.is_some());
        let sent = typed(&mut app, "hi; $(x)");
        assert!(sent.iter().all(Option::is_none), "typing runs nothing");
        assert!(press(&mut app, KeyCode::Enter).is_none());
        let confirm = app.action_footer(200).unwrap().to_string();
        assert_eq!(confirm, "send to api-lead on mini: hi; $(x) · y runs");
        let plan = run_of(press(&mut app, KeyCode::Char('y')));
        assert_eq!(plan.argv[1..], ["--", "api-lead", "hi; $(x)"]);
        assert_eq!(app.going.as_deref(), Some("Send message…"));
        assert!(app.doing.is_none());
    }

    #[test]
    fn anything_but_y_cancels_and_a_y_too_soon_does_too() {
        let mut app = with_actions();
        for other in [
            KeyCode::Char('n'),
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Char('Y'),
        ] {
            typed(&mut app, "xshi");
            press(&mut app, KeyCode::Enter);
            assert!(press(&mut app, other).is_none());
            assert!(app.doing.is_none() && app.going.is_none());
            assert_eq!(app.toast(), Some("Send message cancelled"));
        }
        typed(&mut app, "xshi");
        let t0 = Instant::now() + Duration::from_secs(60);
        app.handle_at(KeyEvent::from(KeyCode::Enter), t0);
        let early = app.handle_at(
            KeyEvent::from(KeyCode::Char('y')),
            t0 + Duration::from_millis(50),
        );
        assert!(early.is_none() && app.going.is_none());
        assert_eq!(
            app.toast(),
            Some("Send message cancelled (keys came too fast)")
        );
    }

    #[test]
    fn esc_in_the_input_line_cancels_and_an_empty_line_asks_again() {
        let mut app = with_actions();
        typed(&mut app, "xs");
        press(&mut app, KeyCode::Enter);
        assert!(app.doing.is_some(), "an empty line stays in the input");
        assert_eq!(app.failed.as_deref(), Some("there is no text to send"));
        press(&mut app, KeyCode::Esc);
        assert!(app.doing.is_none());
        assert_eq!(app.toast(), Some("Send message cancelled"));
    }

    #[test]
    fn y_runs_nothing_when_the_target_changed_after_the_confirm() {
        let mut app = with_actions();
        typed(&mut app, "xshi");
        press(&mut app, KeyCode::Enter);
        let moved = with_sessions(&app, "api-other");
        app.set_rows(moved, hosts());
        assert!(press(&mut app, KeyCode::Char('y')).is_none());
        assert_eq!(
            app.failed.as_deref(),
            Some("Send message cancelled: the target changed")
        );
        assert!(app.going.is_none() && app.doing.is_none());
    }

    #[test]
    fn y_runs_nothing_when_the_row_is_gone_or_lost_its_field() {
        let gone = |change: &dyn Fn(&mut App)| {
            let mut app = with_actions();
            typed(&mut app, "xshi");
            press(&mut app, KeyCode::Enter);
            change(&mut app);
            assert!(press(&mut app, KeyCode::Char('y')).is_none());
            app.failed.unwrap()
        };
        let why = gone(&|app| {
            let rows = app
                .rows
                .iter()
                .filter(|r| r.key != "mini/seat-b")
                .cloned()
                .collect();
            app.set_rows(rows, hosts());
        });
        assert_eq!(why, "Send message cancelled: that row is gone");
        let why = gone(&|app| {
            let mut rows = app.rows.clone();
            rows.iter_mut().for_each(|r| r.ext.clear());
            app.set_rows(rows, hosts());
        });
        assert!(why.contains("does not apply"), "{why}");
    }

    #[test]
    fn x_on_a_row_the_action_does_not_apply_to_says_so() {
        let mut app = with_actions();
        app.selected = Some(Spot::Row("local/seat-a".into()));
        typed(&mut app, "x");
        assert!(app.pending.is_none());
        assert_eq!(app.toast(), Some("no action for this row"));
        typed(&mut app, "s");
        assert!(app.doing.is_none());
        app.selected = Some(Spot::Row("mini/seat-b".into()));
        typed(&mut app, "xq");
        assert_eq!(app.toast(), Some("x takes s; esc cancels"));
        typed(&mut app, "x");
        press(&mut app, KeyCode::Esc);
        assert!(app.pending.is_none() && app.doing.is_none());
    }

    #[test]
    fn typing_into_the_input_line_acts_on_no_pane_and_x_is_not_a_board_key_there() {
        let mut app = with_actions();
        typed(&mut app, "xs");
        let effects = typed(&mut app, "dd yy xs q a j");
        assert!(
            effects.iter().all(Option::is_none),
            "no quit, no close, no answer"
        );
        assert!(app.doing.is_some());
    }

    #[test]
    fn a_second_run_waits_for_the_first() {
        let mut app = with_actions();
        app.going = Some("closing…".into());
        typed(&mut app, "xshi");
        press(&mut app, KeyCode::Enter);
        assert!(press(&mut app, KeyCode::Char('y')).is_none());
        assert_eq!(app.toast(), Some("a jump or close is already running"));
    }
}
