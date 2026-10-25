use ratatui::style::{Color, Modifier, Style};

use crate::board::{Group, Row};

// ANSI named colours only, so the terminal's own theme carries through.
pub const ACCENT: Color = Color::Cyan;
pub const RAISED: Color = Color::DarkGray;

pub fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

pub fn bright() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn header() -> Style {
    Style::new().add_modifier(Modifier::DIM | Modifier::BOLD)
}

/// The glyph for a row: `○` once a done pane has been seen.
pub fn glyph(row: &Row) -> &'static str {
    if row.suspect {
        return "○";
    }
    match row.pane.state.as_str() {
        "blocked" => "◉",
        "error" => "✖",
        "done" if !row.seen => "●",
        "working" => "◐",
        "idle" | "done" => "○",
        _ => "·",
    }
}

pub fn state(row: &Row) -> Style {
    if row.suspect {
        return dim();
    }
    let base = match row.pane.state.as_str() {
        "blocked" | "error" => Style::new().fg(Color::Red),
        "done" if !row.seen => Style::new().fg(Color::Green),
        "working" => Style::new().fg(Color::Yellow),
        _ => dim(),
    };
    if row.stale {
        base.add_modifier(Modifier::DIM)
    } else {
        base
    }
}

pub fn group_title(group: Group) -> &'static str {
    match group {
        Group::NeedsYou => "NEEDS YOU",
        Group::Working => "WORKING",
        Group::Quiet => "QUIET",
        Group::Unreported => "UNREPORTED",
    }
}
