//! The board's one style system: semantic roles for every piece of text, the marks it draws, and
//! a table of how each row state looks. Nothing outside this file picks a colour or a glyph, so a
//! new look is an edit here.

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

/// The board's name and the key table's title.
pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Something wrong: a failed jump or read, a host down, an extension failing, answer mode.
pub fn alert() -> Style {
    Style::new().fg(ALERT)
}

/// A passing message, or a jump on its way.
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
    Blocked,
    Error,
    /// Done and not yet seen.
    Unseen,
    Working,
    /// Idle, or done and seen.
    Quiet,
    /// A record the board doubts.
    Suspect,
    Unknown,
}

#[cfg(test)]
pub const LOOKS: [Look; 7] = [
    Look::Blocked,
    Look::Error,
    Look::Unseen,
    Look::Working,
    Look::Quiet,
    Look::Suspect,
    Look::Unknown,
];

/// What a look draws: its glyph, or the frames it cycles through every `interval`, and its
/// colour and modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Face {
    pub frames: &'static [&'static str],
    pub interval: Duration,
    pub fg: Option<Color>,
    pub modifier: Modifier,
}

impl Face {
    const fn still(glyph: &'static [&'static str], fg: Option<Color>, modifier: Modifier) -> Face {
        Face {
            frames: glyph,
            interval: Duration::ZERO,
            fg,
            modifier,
        }
    }

    pub fn animated(&self) -> bool {
        self.frames.len() > 1 && !self.interval.is_zero()
    }

    /// The frame showing at `now`: from the clock, so every redraw in one interval draws the same.
    pub fn frame(&self, now: DateTime<Utc>) -> &'static str {
        if !self.animated() {
            return self.frames[0];
        }
        let step = now.timestamp_millis().max(0) as u128 / self.interval.as_millis();
        self.frames[(step % self.frames.len() as u128) as usize]
    }
}

/// The table: how each look is drawn.
pub fn face(look: Look) -> Face {
    match look {
        Look::Blocked => Face::still(&["◉"], Some(ALERT), Modifier::empty()),
        Look::Error => Face::still(&["✖"], Some(ALERT), Modifier::empty()),
        Look::Unseen => Face::still(&["●"], Some(OK), Modifier::empty()),
        Look::Working => Face::still(&["◐"], Some(WARN), Modifier::empty()),
        Look::Quiet | Look::Suspect => Face::still(&["○"], None, Modifier::DIM),
        Look::Unknown => Face::still(&["·"], None, Modifier::DIM),
    }
}

pub fn look(row: &Row) -> Look {
    if row.suspect {
        return Look::Suspect;
    }
    match row.pane.state.as_str() {
        "blocked" => Look::Blocked,
        "error" => Look::Error,
        "done" if !row.seen => Look::Unseen,
        "working" => Look::Working,
        "idle" | "done" => Look::Quiet,
        _ => Look::Unknown,
    }
}

/// The glyph for a row at `now`: `○` once a done pane has been seen.
pub fn glyph(row: &Row, now: DateTime<Utc>) -> &'static str {
    face(look(row)).frame(now)
}

pub fn state(row: &Row) -> Style {
    let face = face(look(row));
    let base = Style {
        fg: face.fg,
        ..Style::new().add_modifier(face.modifier)
    };
    if row.stale {
        base.add_modifier(Modifier::DIM)
    } else {
        base
    }
}

/// How often the board must redraw for these looks to move: the shortest interval of the
/// animated ones, or None when none is.
pub fn redraw_every(looks: impl IntoIterator<Item = Look>) -> Option<Duration> {
    looks
        .into_iter()
        .map(face)
        .filter(Face::animated)
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
    fn todays_table_animates_nothing_so_the_board_keeps_its_cadence() {
        for look in LOOKS {
            let face = face(look);
            assert!(!face.animated(), "{look:?}");
            assert_eq!(face.frame(at(0)), face.frame(at(12_345)), "{look:?}");
        }
        assert_eq!(redraw_every(LOOKS), None, "no extra redraws");
    }

    #[test]
    fn an_animated_face_steps_by_the_clock_and_sets_the_redraw() {
        let spinner = Face {
            frames: &["a", "b", "c"],
            interval: Duration::from_millis(100),
            fg: None,
            modifier: Modifier::empty(),
        };
        assert!(spinner.animated());
        assert_eq!(spinner.frame(at(1_000)), "b", "1000 / 100 = 10, 10 % 3 = 1");
        assert_eq!(
            spinner.frame(at(1_099)),
            "b",
            "the same frame for the whole interval"
        );
        assert_eq!(spinner.frame(at(1_100)), "c");
        assert_eq!(spinner.frame(at(1_200)), "a", "and round again");
        let one = Face {
            interval: Duration::from_millis(100),
            ..Face::still(&["x"], None, Modifier::empty())
        };
        assert!(!one.animated(), "one frame never needs a redraw");
    }
}
