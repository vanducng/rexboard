//! The Nerd Font glyphs the card's facts take with `"icons": "nerd"`, each pinned by its Nerd
//! Fonts name and code point. All are single BMP scalars, one cell wide in a Nerd Font Mono;
//! `cod-claude`, `cod-openai`, `cod-robot` and `cod-search_sparkle` need Nerd Fonts 3.5.0.

/// A glyph and the Nerd Fonts name it is pinned to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Icon {
    pub name: &'static str,
    pub glyph: &'static str,
}

const fn icon(name: &'static str, glyph: &'static str) -> Icon {
    Icon { name, glyph }
}

pub const HOST: Icon = icon("cod-server", "\u{eb50}");
pub const TURN: Icon = icon("fa-hourglass_half", "\u{f252}");
pub const TITLE: Icon = icon("cod-note", "\u{eb26}");
pub const SESSION: Icon = icon("cod-layers", "\u{ebd2}");
pub const WINDOW: Icon = icon("cod-window", "\u{eb7f}");
pub const BLOCK: Icon = icon("cod-split_horizontal", "\u{eb56}");
pub const MODEL: Icon = icon("fa-microchip", "\u{f2db}");
pub const CTX: Icon = icon("fa-tachometer", "\u{f0e4}");
pub const TOK: Icon = icon("fa-database", "\u{f1c0}");
/// An extension that declares no icon of its own.
pub const EXTENSION: Icon = icon("oct-package", "\u{f487}");

const CLAUDE: Icon = icon("cod-claude", "\u{ec82}");
const CODEX: Icon = icon("cod-openai", "\u{ec81}");
const OPENCODE: Icon = icon("cod-terminal", "\u{ea85}");
const GEMINI: Icon = icon("cod-sparkle", "\u{ec10}");
const PI: Icon = icon("fae-pi", "\u{e22c}");
const ROBOT: Icon = icon("cod-robot", "\u{ec20}");

const NATIVE: Icon = icon("cod-verified", "\u{eb77}");
const BRIDGE: Icon = icon("cod-plug", "\u{eb2d}");
const INFERRED: Icon = icon("cod-search_sparkle", "\u{ec50}");

/// A harness's mark, by the harness name a pane reports; any other harness is a robot.
pub fn harness(name: Option<&str>) -> Icon {
    match name {
        Some("claude-code" | "claude") => CLAUDE,
        Some("codex") => CODEX,
        Some("opencode") => OPENCODE,
        Some("gemini") => GEMINI,
        Some("pi") => PI,
        _ => ROBOT,
    }
}

/// How a row's status reached the board: its own record, a bridge, or a guess from the pane.
pub fn source(source: &str) -> Option<Icon> {
    match source {
        "native" => Some(NATIVE),
        "bridge" => Some(BRIDGE),
        "inferred" => Some(INFERRED),
        _ => None,
    }
}

#[cfg(test)]
pub const ALL: [Icon; 19] = [
    HOST, TURN, TITLE, SESSION, WINDOW, BLOCK, MODEL, CTX, TOK, EXTENSION, CLAUDE, CODEX, OPENCODE,
    GEMINI, PI, ROBOT, NATIVE, BRIDGE, INFERRED,
];

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn every_icon_is_one_bmp_scalar_one_cell_wide_and_named_once() {
        for icon in ALL {
            let mut chars = icon.glyph.chars();
            let c = chars.next().unwrap();
            assert!(chars.next().is_none(), "{} is one scalar", icon.name);
            assert!(
                ('\u{e000}'..='\u{f8ff}').contains(&c),
                "{} is in the BMP's private use area",
                icon.name
            );
            assert_eq!(icon.glyph.width(), 1, "{}", icon.name);
            assert_eq!(
                ALL.iter().filter(|other| other.glyph == icon.glyph).count(),
                1,
                "{} is used once",
                icon.name
            );
        }
    }

    #[test]
    fn harnesses_and_sources_find_their_marks() {
        assert_eq!(harness(Some("claude-code")), CLAUDE);
        assert_eq!(harness(Some("codex")), CODEX);
        assert_eq!(harness(Some("grok")), ROBOT);
        assert_eq!(harness(None), ROBOT);
        assert_eq!(source("bridge"), Some(BRIDGE));
        assert_eq!(source("other"), None);
    }
}
