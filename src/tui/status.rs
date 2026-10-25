//! `f` and a letter: the board shows one status. Each letter means what the board shows, so a
//! row in UNREPORTED (stale, suspect or with no record) only ever matches `u`.

use crate::board::{Group, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Working,
    Done,
    Question,
    Permission,
    Error,
    Unreported,
    Idle,
}

/// In the order the footer lists them.
pub const ALL: [Status; 7] = [
    Status::Working,
    Status::Done,
    Status::Question,
    Status::Permission,
    Status::Error,
    Status::Unreported,
    Status::Idle,
];

impl Status {
    pub fn from_letter(letter: char) -> Option<Self> {
        ALL.into_iter().find(|status| status.letter() == letter)
    }

    pub fn letter(self) -> char {
        self.name().chars().next().unwrap_or('?')
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Done => "done",
            Self::Question => "question",
            Self::Permission => "permission",
            Self::Error => "error",
            Self::Unreported => "unreported",
            Self::Idle => "idle",
        }
    }

    /// `p` takes every blocked row that is not a question: `permission` is the kind agents
    /// send, and a blocked row with another or no kind still waits on you.
    pub fn matches(self, row: &Row) -> bool {
        if row.group == Group::Unreported {
            return self == Self::Unreported;
        }
        let p = &row.pane;
        let question = p.kind.as_deref() == Some("question");
        match self {
            Self::Working => row.group == Group::Working,
            Self::Done => p.state == "done",
            Self::Question => p.state == "blocked" && question,
            Self::Permission => p.state == "blocked" && !question,
            Self::Error => p.state == "error",
            Self::Idle => p.state == "idle",
            Self::Unreported => false,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::tui::app::App;
    use crate::tui::app::tests::{hosts, press, row};

    /// One row per status, plus the edge cases: a seen done row, a blocked row with no kind, a
    /// stale working row (UNREPORTED) and an idle row, which no letter takes.
    pub fn every_status() -> App {
        let mut ask = row("mini/ask", "blocked", "needs_you", false);
        ask.pane.kind = Some("question".into());
        let mut allow = row("mini/allow", "blocked", "needs_you", false);
        allow.pane.kind = Some("permission".into());
        let mut stale = row("mini/stale", "working", "unreported", true);
        stale.stale = true;
        let mut app = App::default();
        app.set_rows(
            vec![
                ask,
                allow,
                row("mini/odd", "blocked", "needs_you", false),
                row("local/err", "error", "needs_you", false),
                row("local/fresh", "done", "needs_you", false),
                row("mini/codex", "working", "working", true),
                row("local/old", "done", "quiet", true),
                row("local/seat-a", "idle", "quiet", true),
                row("local/shell", "unknown", "unreported", true),
                stale,
            ],
            hosts(),
        );
        app
    }

    fn shown(app: &App) -> Vec<String> {
        let mut keys: Vec<String> = app
            .items()
            .into_iter()
            .filter_map(|item| match item {
                crate::tui::app::Item::Row(i) => Some(app.rows[i].key.clone()),
                crate::tui::app::Item::Header { .. } => None,
            })
            .collect();
        keys.sort();
        keys
    }

    fn pick(app: &mut App, letter: char) {
        press(app, KeyCode::Char('f'));
        press(app, KeyCode::Char(letter));
    }

    #[test]
    fn each_letter_shows_its_status_only() {
        for (letter, want) in [
            ('w', vec!["mini/codex"]),
            ('d', vec!["local/fresh", "local/old"]),
            ('q', vec!["mini/ask"]),
            ('p', vec!["mini/allow", "mini/odd"]),
            ('e', vec!["local/err"]),
            ('u', vec!["local/shell", "mini/stale"]),
            ('i', vec!["local/seat-a"]),
        ] {
            let mut app = every_status();
            pick(&mut app, letter);
            assert_eq!(app.status.map(Status::letter), Some(letter));
            assert_eq!(shown(&app), want, "f {letter}");
        }
    }

    #[test]
    fn f_f_and_esc_clear_it() {
        let all = shown(&every_status());
        let mut app = every_status();
        pick(&mut app, 'e');
        pick(&mut app, 'f');
        assert_eq!((app.status, shown(&app)), (None, all.clone()), "f f");
        pick(&mut app, 'e');
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Esc);
        assert_eq!((app.status, shown(&app)), (None, all.clone()), "f esc");
        pick(&mut app, 'e');
        press(&mut app, KeyCode::Esc);
        assert_eq!((app.status, shown(&app)), (None, all), "esc");
    }

    #[test]
    fn another_key_after_f_keeps_the_filter_and_says_why() {
        let mut app = every_status();
        pick(&mut app, 'w');
        pick(&mut app, 'x');
        assert_eq!(app.status, Some(Status::Working));
        assert!(!app.picking_status);
        assert_eq!(app.toast(), Some("f takes w d q p e u i; f or esc clears"));
    }

    #[test]
    fn ctrl_keys_after_f_keep_their_meaning() {
        let mut app = every_status();
        press(&mut app, KeyCode::Char('f'));
        let quit = app.handle(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(matches!(quit, Some(crate::tui::app::Effect::Quit)));
        assert!(!app.picking_status);
        assert_eq!(app.status, None);
    }

    #[test]
    fn it_combines_with_the_text_filter() {
        let mut app = every_status();
        pick(&mut app, 'p');
        press(&mut app, KeyCode::Char('/'));
        for c in "odd".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(shown(&app), ["mini/odd"]);
    }

    #[test]
    fn counts_follow_the_text_filter() {
        let mut app = every_status();
        let counts = |app: &App| ALL.map(|s| app.status_count(s));
        assert_eq!(counts(&app), [1, 2, 1, 2, 1, 2, 1]);
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char('i'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(counts(&app), [1, 0, 1, 2, 0, 1, 0], "only mini rows");
    }

    #[test]
    fn a_filter_opens_folded_groups_and_digits_count_what_shows() {
        let mut app = every_status();
        press(&mut app, KeyCode::Char('z'));
        assert!(!app.folded.is_empty());
        pick(&mut app, 'd');
        assert!(app.folded.is_empty(), "the done rows are not hidden");
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.current().map(|r| r.key.as_str()), Some("local/fresh"));
    }
}
