//! The Nerd Font glyphs the card's facts take with `"icons": "nerd"`, each pinned by its Nerd
//! Fonts name and code point. All are single BMP scalars, one cell wide in a Nerd Font Mono;
//! `cod-search_sparkle` and the harnesses' `cod-claude` and `cod-openai` need Nerd Fonts 3.5.0.
//! A harness's mark lives on its adapter, `Harness::mark`.

/// A glyph and the Nerd Fonts name it is pinned to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Icon {
    pub name: &'static str,
    pub glyph: &'static str,
}

pub const fn icon(name: &'static str, glyph: &'static str) -> Icon {
    Icon { name, glyph }
}

pub const HOST: Icon = icon("cod-server", "\u{eb50}");
pub const TURN: Icon = icon("fa-hourglass_half", "\u{f252}");
pub const TITLE: Icon = icon("cod-note", "\u{eb26}");
pub const SESSION: Icon = icon("cod-layers", "\u{ebd2}");
pub const WINDOW: Icon = icon("cod-window", "\u{eb7f}");
pub const BLOCK: Icon = icon("cod-split_horizontal", "\u{eb56}");
pub const MODEL: Icon = icon("fa-microchip", "\u{f2db}");
pub const EFFORT: Icon = icon("fa-bolt", "\u{f0e7}");
pub const CTX: Icon = icon("fa-tachometer", "\u{f0e4}");
pub const COST: Icon = icon("fa-dollar", "\u{f155}");
pub const QUOTA: Icon = icon("fa-pie_chart", "\u{f200}");

const NATIVE: Icon = icon("cod-verified", "\u{eb77}");
const BRIDGE: Icon = icon("cod-plug", "\u{eb2d}");
const INFERRED: Icon = icon("cod-search_sparkle", "\u{ec50}");

/// How a row's status reached the board: its own record, a bridge, or a guess from the pane.
pub fn source(source: &str) -> Option<Icon> {
    match source {
        "native" => Some(NATIVE),
        "bridge" => Some(BRIDGE),
        "inferred" => Some(INFERRED),
        _ => None,
    }
}

/// Every glyph the board draws, the harnesses' marks included.
#[cfg(test)]
pub fn all() -> Vec<Icon> {
    let own = [
        HOST, TURN, TITLE, SESSION, WINDOW, BLOCK, MODEL, EFFORT, CTX, COST, QUOTA, NATIVE, BRIDGE,
        INFERRED,
    ];
    let marks = crate::harness::BUILTIN.iter().filter_map(|h| h.mark());
    own.into_iter().chain(marks).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn every_icon_is_one_bmp_scalar_one_cell_wide_and_named_once() {
        let all = all();
        for &icon in &all {
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
                all.iter().filter(|other| other.glyph == icon.glyph).count(),
                1,
                "{} is used once",
                icon.name
            );
        }
    }

    #[test]
    fn sources_find_their_marks() {
        assert_eq!(source("bridge"), Some(BRIDGE));
        assert_eq!(source("other"), None);
    }
}
