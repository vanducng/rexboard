//! The detail pane's width beside the list, and tui.json, which keeps it per machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The narrowest pane, as before it could be resized.
pub const MIN: u16 = 38;
/// What the list keeps beside the pane and its rule: its 100-column layout (100 - 38 - 1).
const LIST: u16 = 61;
/// One `<` or `>`, as a share of the board's width.
const STEP: f64 = 0.05;

/// The pane's columns on a board `board` wide: `share` of it, a third by default, held between
/// MIN and what leaves the list its 61 columns.
pub fn width(board: u16, share: Option<f64>) -> u16 {
    let most = board.saturating_sub(LIST + 1).max(MIN);
    let want = share.map_or(board / 3, |share| {
        (share * f64::from(board))
            .round()
            .clamp(0.0, f64::from(u16::MAX)) as u16
    });
    want.clamp(MIN, most)
}

/// The width whose column set a list `list` wide gets. Beside the pane that is the list plus the
/// narrowest pane and its rule, so a list squeezed to 61 columns keeps the 100-column set and
/// TURN shows only when the list itself has room; with no pane on the right it is the board.
pub fn columns_board(board: u16, list: usize, pick: bool) -> u16 {
    if board >= 100 && !pick {
        u16::try_from(list)
            .unwrap_or(u16::MAX)
            .saturating_add(MIN + 1)
    } else {
        board
    }
}

/// The list's width on a board `board` wide: beside the pane and its rule from 100 columns, else
/// the whole board.
pub fn list(board: u16, share: Option<f64>, pick: bool) -> u16 {
    if board >= 100 && !pick {
        board.saturating_sub(width(board, share) + 1)
    } else {
        board
    }
}

/// The share after one `<` (narrower) or `>` (wider), from the width shown now, so a press at a
/// bound changes nothing.
pub fn step(board: u16, share: Option<f64>, wider: bool) -> f64 {
    let now = width(board, share);
    let by = (STEP * f64::from(board)).round().max(1.0) as u16;
    let next = if wider {
        now.saturating_add(by)
    } else {
        now.saturating_sub(by)
    };
    f64::from(width(board, Some(f64::from(next) / f64::from(board)))) / f64::from(board)
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// The detail pane's share of the board's width; none is the default third.
    #[serde(default)]
    pub detail: Option<f64>,
}

/// A missing or unreadable tui.json is the defaults.
pub fn load(path: &Path) -> Prefs {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Prefs>(&text).ok())
        .filter(|prefs| {
            prefs
                .detail
                .is_none_or(|share| share.is_finite() && share > 0.0)
        })
        .unwrap_or_default()
}

pub fn save(path: &Path, prefs: &Prefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        path,
        serde_json::to_vec(prefs).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_a_third_but_never_under_38() {
        assert_eq!(width(100, None), 38, "a third of 100 is 33");
        assert_eq!(width(140, None), 46);
        assert_eq!(width(200, None), 66);
    }

    #[test]
    fn it_keeps_between_38_and_what_leaves_the_list_61() {
        assert_eq!(width(100, Some(0.9)), 38, "at 100 the list needs it all");
        assert_eq!(width(140, Some(0.9)), 78);
        assert_eq!(width(140, Some(0.01)), 38);
        assert_eq!(width(200, Some(0.9)), 138);
    }

    #[test]
    fn angle_brackets_move_it_5_percent_and_stop_at_the_bounds() {
        let wider = step(200, None, true);
        assert_eq!(width(200, Some(wider)), 76, "66 + 10");
        assert_eq!(width(200, Some(step(200, Some(wider), false))), 66);
        let mut share = None;
        for _ in 0..30 {
            share = Some(step(140, share, true));
        }
        assert_eq!(width(140, share), 78);
        assert_eq!(
            width(140, Some(step(140, share, false))),
            71,
            "one press back counts"
        );
        assert_eq!(width(140, Some(step(140, Some(0.01), false))), 38);
    }

    #[test]
    fn angle_brackets_size_the_pane_and_ask_to_keep_it() {
        use crate::tui::app::Effect;
        use crate::tui::app::tests::{app, press};
        use ratatui::crossterm::event::KeyCode;
        let mut board = app();
        board.width = 200;
        let effect = press(&mut board, KeyCode::Char('>'));
        assert_eq!(width(200, board.detail), 76);
        assert!(matches!(effect, Some(Effect::SaveDetail(share)) if Some(share) == board.detail));
        press(&mut board, KeyCode::Char('<'));
        press(&mut board, KeyCode::Char('<'));
        assert_eq!(width(200, board.detail), 56);
        board.width = 80;
        assert!(press(&mut board, KeyCode::Char('>')).is_none());
        assert_eq!(width(200, board.detail), 56, "unchanged below 100");
        assert_eq!(
            board.toast(),
            Some("< and > size the detail pane on boards wider than 100 columns")
        );
        let mut pinned = app();
        pinned.width = 100;
        assert!(
            press(&mut pinned, KeyCode::Char('>')).is_none(),
            "no save at a bound"
        );
        assert_eq!(
            (pinned.detail, pinned.toast()),
            (None, Some("the detail pane is as wide as it goes"))
        );
        assert!(press(&mut pinned, KeyCode::Char('<')).is_none());
        assert_eq!(
            pinned.toast(),
            Some("the detail pane is as narrow as it goes")
        );
        let mut picking = app();
        picking.pick = true;
        picking.width = 200;
        assert!(press(&mut picking, KeyCode::Char('>')).is_none());
        assert_eq!(
            (picking.detail, picking.toast()),
            (None, None),
            "pick has no pane"
        );
    }

    #[test]
    fn the_board_draws_the_pane_at_its_width() {
        use ratatui::{Terminal, backend::TestBackend};
        let rule_at = |board: &crate::tui::app::App| {
            let mut terminal = Terminal::new(TestBackend::new(200, 30)).unwrap();
            terminal
                .draw(|frame| crate::tui::view::draw(frame, board, chrono::Utc::now()))
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..200u16)
                .find(|&x| buffer[(x, 5)].symbol() == "│")
                .unwrap()
        };
        let mut board = crate::tui::app::tests::app();
        assert_eq!(rule_at(&board), 200 - 66 - 1, "a third by default");
        board.detail = Some(step(200, None, true));
        assert_eq!(rule_at(&board), 200 - 76 - 1, "one > later");
    }

    #[test]
    fn prefs_round_trip_and_bad_files_give_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rexboard/tui.json");
        assert_eq!(load(&path), Prefs::default(), "missing");
        save(&path, &Prefs { detail: Some(0.4) }).unwrap();
        assert_eq!(load(&path), Prefs { detail: Some(0.4) });
        for bad in ["{", "[]", r#"{"detail": "wide"}"#, r#"{"detail": -1}"#] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load(&path), Prefs::default(), "{bad}");
        }
    }
}
