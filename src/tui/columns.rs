//! The list's columns: one table says which columns a board width shows, how wide each is and
//! what goes in it, for the rows and their header alike.

use chrono::{DateTime, Utc};

use super::theme;
use crate::board::Row;
use crate::output::{fit, harness, in_state, state_word, what};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state word and the tab, with the detail pane on the right.
    Wide,
}

pub fn tier(width: u16) -> Tier {
    match width {
        0..60 => Tier::Narrow,
        60..100 => Tier::Mid,
        _ => Tier::Wide,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Col {
    Glyph,
    State,
    Age,
    Harness,
    Host,
    Where,
    Msg,
}

/// How a cell is coloured; the view turns it into a style.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tone {
    State,
    Soft,
    Dim,
}

#[derive(Clone, Copy)]
enum Width {
    /// This many columns, the gap after it included.
    Fixed(usize),
    /// A share of what the fixed columns leave.
    Where,
    Msg,
}

pub struct Column {
    pub id: Col,
    pub header: &'static str,
    /// The narrowest board that shows it.
    min_board: u16,
    width: Width,
    pub tone: Tone,
}

const fn column(id: Col, header: &'static str, min_board: u16, width: Width, tone: Tone) -> Column {
    Column {
        id,
        header,
        min_board,
        width,
        tone,
    }
}

/// Every column, left to right.
pub const COLUMNS: [Column; 7] = [
    column(Col::Glyph, "", 0, Width::Fixed(2), Tone::State),
    column(Col::State, "STATE", 100, Width::Fixed(9), Tone::State),
    column(Col::Age, "AGE", 0, Width::Fixed(7), Tone::Soft),
    column(Col::Harness, "HARNESS", 60, Width::Fixed(8), Tone::Soft),
    column(Col::Host, "HOST", 60, Width::Fixed(8), Tone::Dim),
    column(Col::Where, "WHERE", 0, Width::Where, Tone::Soft),
    column(Col::Msg, "MSG", 0, Width::Msg, Tone::Soft),
];

/// The columns a board `board` columns wide shows in a list `list` columns wide, each with its
/// width.
pub struct Columns {
    pub tier: Tier,
    pub cols: Vec<(&'static Column, usize)>,
}

impl Columns {
    pub fn new(board: u16, list: usize) -> Columns {
        let tier = tier(board);
        let shown: Vec<&Column> = COLUMNS.iter().filter(|c| board >= c.min_board).collect();
        let lead: usize = shown
            .iter()
            .map(|c| match c.width {
                Width::Fixed(n) => n,
                _ => 0,
            })
            .sum();
        let rest = list.saturating_sub(lead);
        let place = if tier == Tier::Wide {
            rest * 5 / 9
        } else {
            rest * 9 / 20
        };
        let cols = shown
            .into_iter()
            .map(|c| {
                let width = match c.width {
                    Width::Fixed(n) => n,
                    Width::Where => place,
                    Width::Msg => rest - place,
                };
                (c, width)
            })
            .collect();
        Columns { tier, cols }
    }

    pub fn width(&self, id: Col) -> usize {
        self.cols
            .iter()
            .find(|(c, _)| c.id == id)
            .map_or(0, |(_, w)| *w)
    }

    pub fn has(&self, id: Col) -> bool {
        self.cols.iter().any(|(c, _)| c.id == id)
    }

    /// One row's cells, each fitted to its column. `place` is its WHERE.
    pub fn cells(&self, row: &Row, place: &str, now: DateTime<Utc>) -> Vec<(String, Tone)> {
        self.cols
            .iter()
            .map(|(c, width)| {
                let text = match c.id {
                    Col::Glyph => theme::glyph(row).to_owned(),
                    Col::State => state_word(row),
                    Col::Age => in_state(row, now),
                    Col::Harness => harness(row),
                    Col::Host => row.pane.host.clone(),
                    Col::Where => place.to_owned(),
                    Col::Msg => return (fit(what(row), *width), c.tone),
                };
                (fit(&text, width.saturating_sub(1)) + " ", c.tone)
            })
            .collect()
    }
}
