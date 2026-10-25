//! The list's columns: one table says which columns a board width shows, how wide each is and
//! what goes in it, for the rows and their header alike.

use chrono::{DateTime, Utc};

use super::theme;
use crate::board::Row;
use crate::output::{fit, harness, in_state, span, state_word, what};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state word with its kind, and the tab, with the detail pane on the
    /// right. TURN joins at 140.
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
    Turn,
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
pub const COLUMNS: [Column; 8] = [
    column(Col::Glyph, "", 0, Width::Fixed(2), Tone::State),
    column(Col::State, "STATE", 100, Width::Fixed(13), Tone::State),
    column(Col::Age, "AGE", 0, Width::Fixed(7), Tone::Soft),
    column(Col::Turn, "TURN", 140, Width::Fixed(7), Tone::Soft),
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
            rest * 2 / 3
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
                    Col::State => state(row),
                    Col::Age => in_state(row, now),
                    Col::Turn => row.last_turn.map_or_else(|| "-".to_owned(), span),
                    Col::Harness => harness(row),
                    Col::Host => row.pane.host.clone(),
                    Col::Where => place.to_owned(),
                    Col::Msg => return (fit(what(row), *width), c.tone),
                };
                (fit(&text, width.saturating_sub(1)) + " ", c.tone)
            })
            .collect()
    }

    /// The header line's cells, at the same widths as the rows'.
    pub fn header(&self) -> Vec<String> {
        self.cols
            .iter()
            .map(|(c, width)| match c.id {
                Col::Msg => fit(c.header, *width),
                _ => fit(c.header, width.saturating_sub(1)) + " ",
            })
            .collect()
    }
}

/// The state word, with the first four letters of its kind: `blocked·perm`.
fn state(row: &Row) -> String {
    let word = state_word(row);
    match &row.pane.kind {
        Some(kind) if !row.suspect && !kind.is_empty() => {
            format!("{word}·{}", kind.chars().take(4).collect::<String>())
        }
        _ => word,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::tui::app::App;
    use crate::tui::app::tests::{app, hosts, row};
    use crate::tui::view::draw;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T05:43:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn buffer(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app, now())).unwrap();
        terminal.backend().buffer().clone()
    }

    fn line(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    /// The list's width beside the detail pane, or the whole width under it.
    fn list_width(app: &App, width: u16) -> usize {
        if width >= 100 && !app.pick {
            usize::from(width) - 39
        } else {
            usize::from(width)
        }
    }

    #[test]
    fn header_cells_start_where_the_row_cells_under_them_start() {
        let mut picking = app();
        picking.pick = true;
        for (app, width) in [
            (app(), 50),
            (app(), 80),
            (app(), 120),
            (app(), 160),
            (picking, 80),
        ] {
            let buf = buffer(&app, width, 30);
            let list = list_width(&app, width);
            let header: String = line(&buf, 1).chars().take(list).collect();
            let columns = Columns::new(width, list);
            let row_y = (2..buf.area.height)
                .find(|&y| line(&buf, y).contains("seat-b msg"))
                .unwrap();
            let mut x = 0u16;
            for (column, w) in &columns.cols {
                if !column.header.is_empty() {
                    let label: String = (x..x + column.header.len() as u16)
                        .map(|x| buf[(x, 1)].symbol())
                        .collect();
                    assert_eq!(label, column.header, "{width}: header at {x}:\n{header}");
                    assert_ne!(
                        buf[(x, row_y)].symbol(),
                        " ",
                        "{width}: {} cell starts at {x}:\n{header}\n{}",
                        column.header,
                        line(&buf, row_y)
                    );
                }
                x += *w as u16;
            }
            let named: Vec<&str> = header.split_whitespace().collect();
            let drawn: Vec<&str> = columns
                .cols
                .iter()
                .map(|(c, _)| c.header)
                .filter(|h| !h.is_empty())
                .collect();
            assert_eq!(
                named, drawn,
                "{width}: the header names exactly the columns drawn"
            );
        }
    }

    #[test]
    fn turn_shows_from_140_columns_with_a_dash_for_no_turn() {
        assert!(!line(&buffer(&app(), 139, 30), 1).contains("TURN"));
        let mut app = App::default();
        let mut timed = row("local/quick", "done", "needs_you", false);
        timed.last_turn = Some(125);
        let untimed = row("local/fresh", "blocked", "needs_you", false);
        app.set_rows(vec![untimed, timed], hosts());
        let buf = buffer(&app, 140, 30);
        let x = line(&buf, 1).find("TURN").unwrap();
        let cell = |needle: &str| {
            let y = (2..30).find(|&y| line(&buf, y).contains(needle)).unwrap();
            line(&buf, y).chars().skip(x).take(6).collect::<String>()
        };
        assert_eq!(cell("quick msg").trim_end(), "2m");
        assert_eq!(cell("fresh msg").trim_end(), "-");
    }

    #[test]
    fn state_carries_its_kind_from_100_columns() {
        let mut app = App::default();
        let mut asking = row("local/asking", "blocked", "needs_you", false);
        asking.pane.kind = Some("permission".into());
        app.set_rows(vec![asking], hosts());
        let text = |width| {
            let buf = buffer(&app, width, 20);
            (0..20)
                .map(|y| line(&buf, y))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(text(100).contains("◉ blocked·perm "), "{}", text(100));
        assert!(!text(99).contains("blocked·perm"), "{}", text(99));
    }

    #[test]
    fn under_12_rows_the_header_gives_its_line_back_to_the_list() {
        let buf = buffer(&app(), 80, 12);
        assert!(line(&buf, 1).contains("AGE"));
        let buf = buffer(&app(), 80, 11);
        assert!(!(0..11).any(|y| line(&buf, y).contains("HARNESS")));
        assert!(
            line(&buf, 1).starts_with("NEEDS YOU (2)"),
            "the list starts right under the top bar:\n{}",
            line(&buf, 1)
        );
    }
}
