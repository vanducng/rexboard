//! The list's columns: one table says which columns a board width shows, how wide each is and
//! what goes in it, for the rows and their header alike.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use super::theme;
use crate::board::Row;
use crate::output::{fit, harness, in_state, labels, path, span, state_word, what};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state (or its kind) and the tab, with the detail pane on the right.
    /// TURN joins at 140.
    Wide,
}

pub fn tier(width: u16) -> Tier {
    match width {
        0..60 => Tier::Narrow,
        60..100 => Tier::Mid,
        _ => Tier::Wide,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// A sort's direction: `Up` is a to z, newest or shortest first, and for STATE most urgent first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
}

impl Dir {
    pub fn arrow(self) -> char {
        match self {
            Dir::Up => '↑',
            Dir::Down => '↓',
        }
    }

    pub fn flip(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

pub type Sort = (Col, Dir);

pub struct Column {
    pub id: Col,
    pub header: &'static str,
    /// Shift and this letter sorts by the column.
    pub letter: Option<char>,
    /// The direction its first press sorts in: the longest wait first for AGE and TURN.
    pub first: Dir,
    /// The narrowest board that shows it.
    min_board: u16,
    width: Width,
    pub tone: Tone,
}

const fn column(
    id: Col,
    header: &'static str,
    letter: Option<char>,
    first: Dir,
    min_board: u16,
    width: Width,
    tone: Tone,
) -> Column {
    Column {
        id,
        header,
        letter,
        first,
        min_board,
        width,
        tone,
    }
}

/// Every column, left to right. Sort letters follow k9s: S state, A age, T turn, R harness
/// (runtime), O host, N where (name), M msg.
pub const COLUMNS: [Column; 8] = [
    column(
        Col::Glyph,
        "",
        None,
        Dir::Up,
        0,
        Width::Fixed(2),
        Tone::State,
    ),
    column(
        Col::State,
        "STATE",
        Some('S'),
        Dir::Up,
        100,
        Width::Fixed(11),
        Tone::State,
    ),
    column(
        Col::Age,
        "AGE",
        Some('A'),
        Dir::Down,
        0,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Turn,
        "TURN",
        Some('T'),
        Dir::Down,
        140,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Harness,
        "HARNESS",
        Some('R'),
        Dir::Up,
        60,
        Width::Fixed(8),
        Tone::Soft,
    ),
    column(
        Col::Host,
        "HOST",
        Some('O'),
        Dir::Up,
        60,
        Width::Fixed(8),
        Tone::Dim,
    ),
    column(
        Col::Where,
        "WHERE",
        Some('N'),
        Dir::Up,
        0,
        Width::Where,
        Tone::Soft,
    ),
    column(
        Col::Msg,
        "MSG",
        Some('M'),
        Dir::Up,
        0,
        Width::Msg,
        Tone::Soft,
    ),
];

pub fn column_of(id: Col) -> &'static Column {
    COLUMNS
        .iter()
        .find(|c| c.id == id)
        .expect("every Col has a column")
}

pub fn by_letter(letter: char) -> Option<&'static Column> {
    COLUMNS.iter().find(|c| c.letter == Some(letter))
}

/// A header cell: its text, and which of its characters to underline (the sort letter).
pub struct HeaderCell {
    pub text: String,
    pub underline: Option<usize>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Num(i64),
    Text(String),
}

/// What a column sorts on. None (no record to date it, no turn yet) sorts last either way.
fn sort_key(id: Col, row: &Row) -> Option<SortKey> {
    let text = |s: &str| Some(SortKey::Text(s.to_lowercase()));
    match id {
        Col::Glyph => None,
        Col::State => Some(SortKey::Num(match state_word(row).as_str() {
            "blocked" => 0,
            "error" => 1,
            "done" => 2,
            "working" => 3,
            "idle" | "idle?" => 4,
            _ => 5,
        })),
        // A bigger age is an older `since`.
        Col::Age => (row.pane.source != "inferred")
            .then(|| DateTime::parse_from_rfc3339(&row.since).ok())
            .flatten()
            .map(|since| SortKey::Num(-since.timestamp_millis())),
        Col::Turn => row.last_turn.map(SortKey::Num),
        Col::Harness => text(&harness(row)),
        Col::Host => text(&row.pane.host),
        Col::Where => text(&path(&labels(row, true), usize::MAX)),
        Col::Msg => text(what(row)),
    }
}

/// Orders two rows by a column, with rows that have no value last in both directions.
pub fn compare(a: &Row, b: &Row, (id, dir): Sort) -> Ordering {
    match (sort_key(id, a), sort_key(id, b)) {
        (Some(x), Some(y)) if dir == Dir::Up => x.cmp(&y),
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

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
        // WHERE names the row, so from 100 it keeps v0.1's width (5/9 of what v0.1 left, which
        // was 2 more before STATE widened) and from 140 TURN's room comes out of MSG.
        let place = match (tier, board) {
            (_, 140..) => rest * 2 / 3,
            (Tier::Wide, _) => ((rest + 2) * 5 / 9).min(rest),
            _ => rest * 9 / 20,
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

    /// The header line's cells, at the same widths as the rows'. Each sort letter is
    /// underlined, and the sorted column's arrow sits in the gap just left of its name, so no
    /// name moves.
    pub fn header(&self, sort: Option<Sort>) -> Vec<HeaderCell> {
        let mut cells: Vec<HeaderCell> = self
            .cols
            .iter()
            .map(|(c, width)| {
                let text = match c.id {
                    Col::Msg => fit(c.header, *width),
                    _ => fit(c.header, width.saturating_sub(1)) + " ",
                };
                let underline = c
                    .letter
                    .and_then(|letter| c.header.find(letter))
                    .filter(|&at| at < text.trim_end().len());
                HeaderCell { text, underline }
            })
            .collect();
        if let Some((id, dir)) = sort
            && let Some(at) = self.cols.iter().position(|(c, _)| c.id == id)
            && at > 0
        {
            let before = &mut cells[at - 1].text;
            before.pop();
            before.push(dir.arrow());
        }
        cells
    }
}

/// The kind when the row has one (`permission`, `question`), since the glyph already says
/// blocked; else the state word.
fn state(row: &Row) -> String {
    match &row.pane.kind {
        Some(kind) if !row.suspect && !kind.is_empty() => kind.clone(),
        _ => state_word(row),
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
    fn state_shows_the_kind_word_from_100_columns() {
        let mut app = App::default();
        let mut asking = row("local/ask", "blocked", "needs_you", false);
        asking.pane.kind = Some("permission".into());
        let mut choosing = row("local/pick", "blocked", "needs_you", false);
        choosing.pane.kind = Some("question".into());
        let plain = row("local/plain", "blocked", "needs_you", false);
        let mut stale = row("local/stale", "working", "unreported", true);
        stale.pane.kind = Some("permission".into());
        stale.suspect = true;
        app.set_rows(vec![asking, choosing, plain, stale], hosts());
        let row_at = |width: u16, needle: &str| {
            let buf = buffer(&app, width, 30);
            let y = (2..30).find(|&y| line(&buf, y).contains(needle)).unwrap();
            let list = list_width(&app, width);
            line(&buf, y).chars().take(list).collect::<String>()
        };
        assert!(row_at(100, "ask msg").starts_with("◉ permission "));
        assert!(row_at(100, "pick msg").starts_with("◉ question "));
        assert!(row_at(100, "plain msg").starts_with("◉ blocked "));
        assert!(row_at(100, "stale msg").starts_with("○ idle? "));
        let narrow = row_at(99, "ask msg");
        assert!(
            narrow.starts_with("◉ 5h43m "),
            "no STATE below 100: {narrow}"
        );
        assert!(!narrow.contains("permission") && !narrow.contains("blocked"));
    }

    #[test]
    fn where_keeps_v0_1_width_from_100_and_takes_two_thirds_from_140() {
        let at = |board: u16| {
            let columns = Columns::new(board, usize::from(board) - 39);
            (columns.width(Col::Where), columns.width(Col::Msg))
        };
        assert_eq!(at(100), (15, 10), "v0.1's WHERE at 100");
        assert_eq!(at(120), (26, 19), "v0.1's WHERE at 120");
        assert_eq!(at(140), (38, 20), "from 140 WHERE takes two thirds");
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

    #[test]
    fn the_sorted_column_gets_an_arrow_in_the_gap_and_each_letter_is_underlined() {
        let columns = Columns::new(120, 81);
        let plain: Vec<String> = columns.header(None).into_iter().map(|c| c.text).collect();
        let cells = columns.header(Some((Col::Age, Dir::Down)));
        let text: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(text[1], "STATE     ↓", "the arrow takes the gap before AGE");
        assert_eq!(text[2], plain[2], "AGE itself does not move");
        assert_eq!(
            text.concat().chars().count(),
            plain.concat().chars().count()
        );
        let underlined: Vec<Option<char>> = cells
            .iter()
            .map(|c| c.underline.map(|at| c.text.as_bytes()[at] as char))
            .collect();
        assert_eq!(
            underlined,
            [
                None,
                Some('S'),
                Some('A'),
                Some('R'),
                Some('O'),
                None,
                Some('M')
            ],
            "WHERE has no N to underline"
        );
    }
}
