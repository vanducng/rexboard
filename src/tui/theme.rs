//! The board's one style system: semantic roles for every piece of text, the marks it draws, and
//! a table of how each row state looks. Nothing outside this file picks a colour or a glyph, so a
//! new look is an edit here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use ratatui::style::{Color, Modifier, Style};

use crate::board::{Group, Row};

// ANSI named colours only, so the terminal's own theme carries through.
const ACCENT: Color = Color::Cyan;
const RAISED: Color = Color::DarkGray;
const ALERT: Color = Color::Red;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;

/// The card's and the full view's bar, and the answer bar.
pub const RULE: &str = "━";
/// Each end of a bar.
pub const BAR_END: &str = "━━";
/// Above the footer while the card is hidden.
pub const LINE: &str = "─";
/// After a group header's title.
pub const GROUP_FILL: &str = "┄";
pub const FOLDED: &str = "▸ ";
/// Before the count of rows below the list.
pub const MORE: &str = "↓";
/// The end of a prompt's text.
pub const CURSOR: &str = "▏";
/// Before answer mode's reply line.
pub const PROMPT: &str = "›";
/// The end of a pane line cut at the edge.
pub const CUT: &str = "…";
/// Before each line of a pane's tail in the card.
pub const GUTTER: &str = " │ ";
pub const SORT_UP: char = '↑';
pub const SORT_DOWN: char = '↓';

/// Text as it is: a message's body, a plain row.
pub fn plain() -> Style {
    Style::new()
}

/// Text that steps back: labels, hints, separators, and quiet rows.
pub fn quiet() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

/// Text that matters: a headline, a prompt, a key.
pub fn strong() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// The board's name, the key table's title, and an answer armed to send.
pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Something wrong: a failed jump or read, a host down, an extension failing, answer mode.
pub fn alert() -> Style {
    Style::new().fg(ALERT)
}

/// A passing message, or a jump or an answer on its way.
pub fn note() -> Style {
    Style::new().fg(ACCENT)
}

/// The column header and the group headers.
pub fn header() -> Style {
    Style::new().add_modifier(Modifier::DIM | Modifier::BOLD)
}

/// A sort or status letter in `base` text.
pub fn letter(base: Style) -> Style {
    base.add_modifier(Modifier::UNDERLINED)
}

/// The selected row or group header.
pub fn selected(base: Style) -> Style {
    base.bg(RAISED)
}

/// The mark at a cut pane line, on the band `bg` the line was drawn on, if any.
pub fn cut_mark(bg: Option<Color>) -> Style {
    bg.map_or_else(quiet, |bg| quiet().bg(bg))
}

/// The row's name and state on a bar: its state's colour, bold.
pub fn bar_label(row: &Row) -> Style {
    state(row).add_modifier(Modifier::BOLD)
}

/// The answer bar's label.
pub fn alert_label() -> Style {
    alert().add_modifier(Modifier::BOLD)
}

/// How a row looks, before the table says what that means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// Blocked on a permission, or on anything but a question.
    Blocked,
    Question,
    Error,
    /// Done and not yet seen.
    Unseen,
    Seen,
    Working,
    Idle,
    /// A `working` record the board doubts.
    Suspect,
    /// No state, or one only inferred from the pane.
    Unknown,
    /// Its host stopped answering.
    Stale,
}

#[cfg(test)]
pub const LOOKS: [Look; 10] = [
    Look::Blocked,
    Look::Question,
    Look::Error,
    Look::Unseen,
    Look::Seen,
    Look::Working,
    Look::Idle,
    Look::Suspect,
    Look::Unknown,
    Look::Stale,
];

/// What a look draws: the frames it cycles through every `interval` (one, for most), the glyph
/// it holds still when motion is off, and its colour and modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Face {
    pub frames: &'static [&'static str],
    pub interval: Duration,
    pub still: &'static str,
    pub fg: Option<Color>,
    pub modifier: Modifier,
}

impl Face {
    const fn of(glyph: &'static str, fg: Option<Color>, modifier: Modifier) -> Face {
        Face {
            frames: &[],
            interval: Duration::ZERO,
            still: glyph,
            fg,
            modifier,
        }
    }

    pub fn animated(&self) -> bool {
        self.frames.len() > 1 && !self.interval.is_zero()
    }

    /// The frame showing at `now`: from the clock, so every redraw in one interval draws the same.
    pub fn frame(&self, now: DateTime<Utc>, moving: bool) -> &'static str {
        if !moving || !self.animated() {
            return self.still;
        }
        let step = now.timestamp_millis().max(0) as u128 / self.interval.as_millis();
        self.frames[(step % self.frames.len() as u128) as usize]
    }
}

/// The table: how each look is drawn. Each glyph is one cell wide in every terminal and never
/// an emoji, and its shape says the state without the colour.
pub fn face(look: Look) -> Face {
    let loud = Modifier::BOLD;
    match look {
        Look::Blocked => Face::of("!", Some(ALERT), loud),
        Look::Question => Face::of("?", Some(ALERT), loud),
        Look::Error => Face::of("✗", Some(ALERT), loud),
        Look::Unseen => Face::of("✓", Some(OK), loud),
        Look::Seen => Face::of("✓", None, Modifier::DIM),
        Look::Working => Face {
            frames: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            interval: Duration::from_millis(160),
            ..Face::of("⠿", Some(WARN), Modifier::empty())
        },
        Look::Idle | Look::Suspect => Face::of("○", None, Modifier::DIM),
        Look::Unknown => Face::of("·", None, Modifier::DIM),
        Look::Stale => Face::of("◌", None, Modifier::DIM),
    }
}

pub fn look(row: &Row) -> Look {
    if row.stale {
        return Look::Stale;
    }
    if row.suspect {
        return Look::Suspect;
    }
    if row.pane.source == "inferred" {
        return Look::Unknown;
    }
    match row.pane.state.as_str() {
        "blocked" if row.pane.kind.as_deref() == Some("question") => Look::Question,
        "blocked" => Look::Blocked,
        "error" => Look::Error,
        "done" if row.seen => Look::Seen,
        "done" => Look::Unseen,
        "working" => Look::Working,
        "idle" => Look::Idle,
        _ => Look::Unknown,
    }
}

static MOTION: AtomicBool = AtomicBool::new(true);

/// Off holds every animated glyph still: tui.json's `"motion": "reduced"` or
/// REXBOARD_REDUCED_MOTION=1.
pub fn set_motion(on: bool) {
    MOTION.store(on, Ordering::Relaxed);
}

pub fn motion() -> bool {
    MOTION.load(Ordering::Relaxed)
}

/// The glyph for a row at `now`.
pub fn glyph(row: &Row, now: DateTime<Utc>) -> &'static str {
    face(look(row)).frame(now, motion())
}

pub fn state(row: &Row) -> Style {
    let face = face(look(row));
    Style {
        fg: face.fg,
        ..Style::new().add_modifier(face.modifier)
    }
}

/// How often the board must redraw for these looks to move: the shortest interval of the
/// animated ones, or None when none is or motion is off.
pub fn redraw_every(looks: impl IntoIterator<Item = Look>, moving: bool) -> Option<Duration> {
    looks
        .into_iter()
        .map(face)
        .filter(|face| moving && face.animated())
        .map(|face| face.interval)
        .min()
}

pub fn group_title(group: Group) -> &'static str {
    match group {
        Group::NeedsYou => "NEEDS YOU",
        Group::Working => "WORKING",
        Group::Quiet => "QUIET",
        Group::Unreported => "UNREPORTED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(millis).unwrap()
    }

    /// Every colour, style and drawn mark comes from here. vt.rs is exempt: it turns the pane's
    /// own colours into styles. Pane text the board reads (`'━'` in screen.rs) is a char, not a
    /// string, so it does not count.
    #[test]
    fn nothing_outside_the_theme_picks_a_colour_or_a_glyph() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui");
        let picks = [
            "Color::",
            "Style::new()",
            "\"◉",
            "\"✖",
            "\"●",
            "\"◐",
            "\"○",
            "\"·\"",
            "\"✓",
            "\"✗",
            "\"◌",
            "\"⠋",
            "\"⠿",
            "\"━",
            "\"─",
            "\"┄",
            "\"▸",
            "\"▏",
            "\"↓",
            "\"↑",
            "'↑'",
            "'↓'",
            "\" │ ",
            "\" › ",
        ];
        let mut seen = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".rs") || name == "theme.rs" || name == "vt.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap();
            for (n, line) in code.lines().enumerate() {
                let line = line.trim_start();
                if line.starts_with("//") {
                    continue;
                }
                for pick in picks.iter().filter(|p| line.contains(**p)) {
                    seen.push(format!("{name}:{}: {pick}", n + 1));
                }
            }
        }
        assert!(seen.is_empty(), "picked outside the theme: {seen:#?}");
    }

    #[test]
    fn only_working_moves_and_motion_off_holds_it_still() {
        for look in LOOKS {
            let face = face(look);
            assert_eq!(face.animated(), look == Look::Working, "{look:?}");
        }
        let working = face(Look::Working);
        assert_eq!(working.frame(at(0), true), "⠋");
        assert_eq!(working.frame(at(159), true), "⠋");
        assert_eq!(working.frame(at(160), true), "⠙", "one step every 160 ms");
        assert_eq!(
            working.frame(at(1600), true),
            "⠋",
            "ten frames, then round again"
        );
        assert_eq!(working.frame(at(160), false), "⠿", "reduced motion");
        assert_eq!(redraw_every(LOOKS, true), Some(Duration::from_millis(160)));
        assert_eq!(redraw_every(LOOKS, false), None, "reduced motion");
        let resting = LOOKS.into_iter().filter(|look| *look != Look::Working);
        assert_eq!(redraw_every(resting, true), None, "nothing working");
    }

    #[test]
    fn every_state_has_its_glyph() {
        use crate::tui::app::tests::row;
        let glyph_of = |row: &Row| face(look(row)).frame(at(0), false);
        let mut question = row("a/q", "blocked", "needs_you", false);
        question.pane.kind = Some("question".into());
        let mut permission = row("a/p", "blocked", "needs_you", false);
        permission.pane.kind = Some("permission".into());
        let mut inferred = row("a/i", "working", "unreported", false);
        inferred.pane.source = "inferred".into();
        let mut suspect = row("a/s", "working", "unreported", false);
        suspect.suspect = true;
        let mut stale = row("a/t", "blocked", "unreported", false);
        stale.stale = true;
        let cases = [
            (permission, "!"),
            (row("a/b", "blocked", "needs_you", false), "!"),
            (question, "?"),
            (row("a/e", "error", "needs_you", false), "✗"),
            (row("a/d", "done", "needs_you", false), "✓"),
            (row("a/f", "done", "quiet", true), "✓"),
            (row("a/w", "working", "working", false), "⠿"),
            (row("a/z", "idle", "quiet", false), "○"),
            (suspect, "○"),
            (inferred, "·"),
            (row("a/u", "unknown", "unreported", false), "·"),
            (stale, "◌"),
        ];
        for (row, want) in &cases {
            assert_eq!(glyph_of(row), *want, "{}", row.key);
        }
        let unseen = state(&cases[4].0);
        let seen = state(&cases[5].0);
        assert_eq!(unseen.fg, Some(OK), "done and not seen is green");
        assert!(unseen.add_modifier.contains(Modifier::BOLD));
        assert_eq!(seen.fg, None, "seen fades");
        assert!(seen.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn an_animated_face_steps_by_the_clock_and_sets_the_redraw() {
        let spinner = Face {
            frames: &["a", "b", "c"],
            interval: Duration::from_millis(100),
            ..Face::of("s", None, Modifier::empty())
        };
        assert!(spinner.animated());
        assert_eq!(
            spinner.frame(at(1_000), true),
            "b",
            "1000 / 100 = 10, 10 % 3 = 1"
        );
        assert_eq!(
            spinner.frame(at(1_099), true),
            "b",
            "the same frame for the whole interval"
        );
        assert_eq!(spinner.frame(at(1_100), true), "c");
        assert_eq!(spinner.frame(at(1_200), true), "a", "and round again");
        let one = Face {
            frames: &["x"],
            interval: Duration::from_millis(100),
            ..Face::of("x", None, Modifier::empty())
        };
        assert!(!one.animated(), "one frame never needs a redraw");
    }
}
