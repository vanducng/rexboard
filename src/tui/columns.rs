//! The list's columns: one table says which columns a board width shows, how wide each is and
//! what goes in it, for the rows and their header alike.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use super::theme;
use crate::board::Row;
use crate::client::Field;
use crate::output::{
    clean, fit, harness, in_state, labels, name, path, span, squeeze, state_word, turn_msg,
};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state (or its kind) and the tab.
    /// From 132 WHERE splits into SESSION, WINDOW and BLOCK; TURN joins at 140.
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
    Session,
    Window,
    Block,
    Model,
    Ctx,
    Tok,
    Msg,
    /// An extension field, by its id (`<extension>.<field>`).
    Field(&'static str),
}

/// How a cell is coloured; the view turns it into a style.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tone {
    State,
    Soft,
    Dim,
    /// How full the row's context window is: warn, then alert.
    Fill,
}

#[derive(Clone, Copy)]
enum Width {
    /// This many columns, the gap after it included.
    Fixed(usize),
    /// A share of what the fixed columns leave: WHERE's, one of its three parts, or MSG's.
    Where,
    Part(usize),
    Msg,
}

/// The board width from which WHERE splits into SESSION, WINDOW and BLOCK.
pub const SPLIT: u16 = 132;
/// Extension field columns show from this width, and all of them from FIELDS_ALL; below it only
/// the first two.
pub const FIELDS_FROM: u16 = 120;
const FIELDS_ALL: u16 = 160;
/// A field column's width, the gap after it included.
const FIELD_WIDTH: usize = 12;
const ALWAYS: RangeInclusive<u16> = 0..=u16::MAX;
/// MODEL, CTX and TOK show from this width, and only while some row reports its usage.
pub const USAGE_FROM: u16 = 140;
const USAGE: [Col; 3] = [Col::Model, Col::Ctx, Col::Tok];

pub fn is_usage(id: Col) -> bool {
    USAGE.contains(&id)
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
            Dir::Up => theme::SORT_UP,
            Dir::Down => theme::SORT_DOWN,
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
    /// `o` and this letter sorts by the column.
    pub letter: Option<char>,
    /// The direction its first press sorts in: the longest wait first for AGE and TURN.
    pub first: Dir,
    /// The boards that show it.
    boards: RangeInclusive<u16>,
    width: Width,
    pub tone: Tone,
}

const fn column(
    id: Col,
    header: &'static str,
    letter: Option<char>,
    first: Dir,
    boards: RangeInclusive<u16>,
    width: Width,
    tone: Tone,
) -> Column {
    Column {
        id,
        header,
        letter,
        first,
        boards,
        width,
        tone,
    }
}

/// Every column, left to right. Sort letters, after `o`: s state, a age, t turn, h harness, o host,
/// w where, e session, n window, b block, d model, x ctx, k tok, m msg. The usage letters are rare
/// first letters, so an extension field keeps its own.
pub const COLUMNS: [Column; 14] = [
    column(
        Col::Glyph,
        "",
        None,
        Dir::Up,
        ALWAYS,
        Width::Fixed(2),
        Tone::State,
    ),
    column(
        Col::State,
        "STATE",
        Some('s'),
        Dir::Up,
        100..=u16::MAX,
        Width::Fixed(11),
        Tone::State,
    ),
    column(
        Col::Age,
        "AGE",
        Some('a'),
        Dir::Down,
        ALWAYS,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Turn,
        "TURN",
        Some('t'),
        Dir::Down,
        140..=u16::MAX,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Harness,
        "HARNESS",
        Some('h'),
        Dir::Up,
        60..=u16::MAX,
        Width::Fixed(8),
        Tone::Soft,
    ),
    column(
        Col::Host,
        "HOST",
        Some('o'),
        Dir::Up,
        60..=u16::MAX,
        Width::Fixed(8),
        Tone::Dim,
    ),
    column(
        Col::Where,
        "WHERE",
        Some('w'),
        Dir::Up,
        0..=SPLIT - 1,
        Width::Where,
        Tone::Soft,
    ),
    column(
        Col::Session,
        "SESSION",
        Some('e'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(0),
        Tone::Soft,
    ),
    column(
        Col::Window,
        "WINDOW",
        Some('n'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(1),
        Tone::Soft,
    ),
    column(
        Col::Block,
        "BLOCK",
        Some('b'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(2),
        Tone::Soft,
    ),
    column(
        Col::Model,
        "MODEL",
        Some('d'),
        Dir::Up,
        USAGE_FROM..=u16::MAX,
        Width::Fixed(13),
        Tone::Soft,
    ),
    column(
        Col::Ctx,
        "CTX",
        Some('x'),
        Dir::Down,
        USAGE_FROM..=u16::MAX,
        Width::Fixed(5),
        Tone::Fill,
    ),
    column(
        Col::Tok,
        "TOK",
        Some('k'),
        Dir::Down,
        USAGE_FROM..=u16::MAX,
        Width::Fixed(6),
        Tone::Soft,
    ),
    column(
        Col::Msg,
        "MSG",
        Some('m'),
        Dir::Up,
        ALWAYS,
        Width::Msg,
        Tone::Soft,
    ),
];

pub fn column_of(id: Col) -> &'static Column {
    COLUMNS
        .iter()
        .find(|c| c.id == id)
        .or_else(|| field_columns().into_iter().find(|c| c.id == id))
        .expect("every Col has a column")
}

/// The built-in column a sort letter names; `App::column_by_letter` adds the field columns.
pub fn by_letter(letter: char) -> Option<&'static Column> {
    COLUMNS.iter().find(|c| c.letter == Some(letter))
}

// ponytail: field columns are leaked once per distinct (id, title, letter), a handful per run,
// so they share the built-ins' `&'static Column`; a registry with lifetimes if that ever grows.
type FieldKey = (String, String, Option<char>);
static FIELDS: Mutex<BTreeMap<FieldKey, &'static Column>> = Mutex::new(BTreeMap::new());

/// The column for an extension field, titled from its manifest.
pub fn field_column(field: &Field, letter: Option<char>) -> &'static Column {
    let (id, title) = (&field.id, &field.title);
    let mut fields = FIELDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fields
        .entry((id.clone(), title.clone(), letter))
        .or_insert_with(|| {
            let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
            let header: &'static str = Box::leak(title.to_owned().into_boxed_str());
            Box::leak(Box::new(column(
                Col::Field(id),
                header,
                letter,
                Dir::Up,
                FIELDS_FROM..=u16::MAX,
                Width::Fixed(FIELD_WIDTH),
                Tone::Soft,
            )))
        })
}

fn field_columns() -> Vec<&'static Column> {
    let fields = FIELDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fields.values().copied().collect()
}

/// The sort letter for a field named `name`: its first letter no other column has.
pub fn free_letter(name: &str, taken: &[char]) -> Option<char> {
    name.chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .find(|c| by_letter(*c).is_none() && !taken.contains(c))
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
        Col::Session => text(&row.pane.session),
        Col::Window => text(&row.pane.tab),
        Col::Block => text(&name(row)),
        Col::Model => row.usage.as_ref()?.model.as_deref().and_then(text),
        Col::Ctx => Some(SortKey::Num(row.usage.as_ref()?.ctx_pct?.into())),
        Col::Tok => Some(SortKey::Num(row.usage.as_ref()?.tok?.try_into().ok()?)),
        Col::Msg => text(turn_msg(row)),
        Col::Field(id) => row.ext.get(id).and_then(|value| text(value)),
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

/// A usage figure, or `-` when the row has none: blank is blank, never zero.
fn usage(row: &Row, figure: impl Fn(&crate::harness::usage::Usage) -> Option<String>) -> String {
    row.usage
        .as_ref()
        .and_then(figure)
        .unwrap_or_else(|| "-".to_owned())
}

/// The columns a board `board` columns wide shows in a list `list` columns wide, each with its
/// width.
pub struct Columns {
    pub tier: Tier,
    pub cols: Vec<(&'static Column, usize)>,
}

impl Columns {
    /// `fields` are the extension field columns to show after HOST, in order; from FIELDS_FROM
    /// columns, and only the first two below FIELDS_ALL. No MODEL, CTX or TOK.
    #[cfg(test)]
    pub fn new(board: u16, list: usize, fields: &[&'static Column]) -> Columns {
        Self::with_usage(board, list, fields, false)
    }

    /// As `new`, with MODEL, CTX and TOK when `usage`: some row reports what its turn used.
    pub fn with_usage(board: u16, list: usize, fields: &[&'static Column], usage: bool) -> Columns {
        let tier = tier(board);
        let fields = match board {
            ..FIELDS_FROM => &[][..],
            FIELDS_FROM..FIELDS_ALL => &fields[..fields.len().min(2)],
            _ => fields,
        };
        let mut shown: Vec<&'static Column> = COLUMNS
            .iter()
            .filter(|c| c.boards.contains(&board) && (usage || !USAGE.contains(&c.id)))
            .collect();
        if let Some(host) = shown.iter().position(|c| c.id == Col::Host) {
            shown.splice(host + 1..host + 1, fields.iter().copied());
        }
        let lead: usize = shown
            .iter()
            .map(|c| match c.width {
                Width::Fixed(n) => n,
                _ => 0,
            })
            .sum();
        let rest = list.saturating_sub(lead);
        // WHERE names the row, so from 100 it keeps v0.1's width (5/9 of what v0.1 left, which
        // was 2 more before STATE widened); once split its parts take two thirds, and from 140
        // TURN's room comes out of MSG.
        let place = match (tier, board) {
            (_, SPLIT..) => rest * 2 / 3,
            (Tier::Wide, _) => ((rest + 2) * 5 / 9).min(rest),
            _ => rest * 9 / 20,
        };
        let cols = shown
            .into_iter()
            .map(|c| {
                let width = match c.width {
                    Width::Fixed(n) => n,
                    Width::Where => place,
                    Width::Part(2) => place - 2 * (place / 3),
                    Width::Part(_) => place / 3,
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

    /// The sortable column a click at `x` on the header means: the sort arrow means the column it
    /// marks, else the column whose cell spans `x`.
    pub fn at(&self, x: usize, sort: Option<Sort>) -> Option<&'static Column> {
        if let Some((at, id, _)) = self.arrow(sort)
            && at == x
        {
            return Some(column_of(id));
        }
        let mut start = 0;
        for (c, width) in &self.cols {
            if x < start + width {
                return c.letter.map(|_| *c);
            }
            start += width;
        }
        None
    }

    pub fn has(&self, id: Col) -> bool {
        self.cols.iter().any(|(c, _)| c.id == id)
    }

    /// One row's cells, each fitted to its column. `place` is its WHERE, or its BLOCK once WHERE
    /// splits; a long session, window or block keeps both its ends.
    pub fn cells(&self, row: &Row, place: &str, now: DateTime<Utc>) -> Vec<(String, Tone)> {
        self.cols
            .iter()
            .map(|(c, width)| {
                let text = match c.id {
                    Col::Glyph => theme::glyph(row, now).to_owned(),
                    Col::State => state(row),
                    Col::Age => in_state(row, now),
                    Col::Turn => row.last_turn.map_or_else(|| "-".to_owned(), span),
                    Col::Harness => harness(row),
                    Col::Host => row.pane.host.clone(),
                    Col::Where => place.to_owned(),
                    Col::Session => squeeze(&clean(&row.pane.session), width.saturating_sub(1)),
                    Col::Window => squeeze(&clean(&row.pane.tab), width.saturating_sub(1)),
                    Col::Block => squeeze(place, width.saturating_sub(1)),
                    Col::Model => usage(row, |u| u.model.clone()),
                    Col::Ctx => usage(row, |u| u.ctx_pct.map(|p| format!("{p}%"))),
                    Col::Tok => usage(row, |u| u.tok.map(crate::output::tokens)),
                    Col::Msg => return (fit(turn_msg(row), *width), c.tone),
                    Col::Field(id) => row.ext.get(id).cloned().unwrap_or_else(|| "-".to_owned()),
                };
                (fit(&text, width.saturating_sub(1)) + " ", c.tone)
            })
            .collect()
    }

    /// Where the sorted column's arrow goes, as a column on the line: in the gap just left of its
    /// name, so no name moves, unless that gap is a single space, where it would join the two
    /// names; then just after the name, when the cell has room.
    fn arrow(&self, sort: Option<Sort>) -> Option<(usize, Col, char)> {
        let (id, dir) = sort?;
        let at = self.cols.iter().position(|(c, _)| c.id == id)?;
        let start: usize = self.cols[..at].iter().map(|(_, w)| w).sum();
        let (column, width) = self.cols[at];
        let room = if id == Col::Msg {
            width
        } else {
            width.saturating_sub(1)
        };
        let gap = at > 0 && {
            let (before, before_width) = self.cols[at - 1];
            before.header.len() + 1 < before_width
        };
        if !gap && column.header.len() < room {
            Some((start + column.header.len(), id, dir.arrow()))
        } else {
            start.checked_sub(1).map(|x| (x, id, dir.arrow()))
        }
    }

    /// The header line's cells, at the same widths as the rows'. Each sort letter is
    /// underlined, and the sorted column's arrow is placed as `arrow` says.
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
                    .and_then(|letter| c.header.find(letter.to_ascii_uppercase()))
                    .filter(|_| text.starts_with(c.header));
                HeaderCell { text, underline }
            })
            .collect();
        if let Some((x, _, arrow)) = self.arrow(sort) {
            let mut start = 0;
            for (cell, (_, width)) in cells.iter_mut().zip(&self.cols) {
                if x < start + width {
                    let mut chars: Vec<char> = cell.text.chars().collect();
                    chars[x - start] = arrow;
                    cell.text = chars.into_iter().collect();
                    break;
                }
                start += width;
            }
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
    use crate::tui::app::testkit::{app, hosts, row};
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
            let list = usize::from(width);
            let header: String = line(&buf, 2).chars().take(list).collect();
            let columns = Columns::new(width, list, &[]);
            let row_y = (3..buf.area.height)
                .find(|&y| line(&buf, y).contains("seat-b msg"))
                .unwrap();
            let mut x = 0u16;
            for (column, w) in &columns.cols {
                if !column.header.is_empty() {
                    let label: String = (x..x + column.header.len() as u16)
                        .map(|x| buf[(x, 2)].symbol())
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
    fn a_click_on_each_header_cell_sorts_by_its_column_in_the_table_view() {
        for width in [100, 140, 200] {
            let mut app = app();
            app.view = crate::tui::views::views().remove(1);
            (app.width, app.height) = (width, 30);
            let header = line(&buffer(&app, width, 30), 2);
            let columns = Columns::new(width, usize::from(width), &[]);
            for (column, _) in columns.cols.iter().filter(|(c, _)| c.letter.is_some()) {
                let x = header.find(column.header).unwrap();
                for at in [x, x + column.header.len() - 1] {
                    app.sort = None;
                    app.click(at as u16, 2);
                    assert_eq!(
                        app.sort,
                        Some((column.id, column.first)),
                        "{width}: {} at {at}:\n{header}",
                        column.header
                    );
                }
            }
            app.sort = None;
            app.click(3, 5);
            assert_eq!(app.sort, None, "{width}: a click on a row does not sort");
        }
        let mut status = app();
        (status.width, status.height) = (140, 30);
        status.click(10, 2);
        assert_eq!(status.sort, None, "clicks sort only in the table view");
    }

    fn table(width: u16) -> App {
        let mut app = app();
        app.view = crate::tui::views::views().remove(1);
        (app.width, app.height) = (width, 30);
        app
    }

    #[test]
    fn a_click_on_the_sort_arrow_flips_the_sort() {
        use crate::tui::app::testkit::sort;

        for (width, letter, sorted) in [(140, 'a', Col::Age), (100, 'o', Col::Host)] {
            let mut app = table(width);
            sort(&mut app, letter);
            let first = app.sort;
            let header = line(&buffer(&app, width, 30), 2);
            let x = header.chars().position(|c| c == '↑' || c == '↓').unwrap();
            app.click(x as u16, 2);
            assert_eq!(
                app.sort,
                first.map(|(id, dir)| (id, dir.flip())),
                "{width}: the arrow flips {sorted:?}:\n{header}"
            );
        }
    }

    #[test]
    fn at_100_the_arrow_follows_a_name_with_no_gap_before_it() {
        use crate::tui::app::testkit::sort;

        let mut app = table(100);
        sort(&mut app, 'o');
        let header = line(&buffer(&app, 100, 30), 2);
        assert!(header.contains("HARNESS HOST↑"), "{header}");
        sort(&mut app, 'a');
        let header = line(&buffer(&app, 100, 30), 2);
        assert!(
            header.contains("STATE     ↓AGE"),
            "a gap keeps it before the name: {header}"
        );
    }

    #[test]
    fn a_clicked_row_in_a_scrolled_list_stays_under_the_pointer() {
        use crate::tui::app::Spot;
        let mut board = App::default();
        board.set_rows(
            (0..40)
                .map(|n| row(&format!("local/r{n:02}"), "idle", "quiet", true))
                .collect(),
            hosts(),
        );
        board.view = crate::tui::views::views().remove(1);
        (board.width, board.height) = (140, 20);
        board.selected = Some(Spot::Row("local/r30".into()));
        let y_of = |board: &App, key: &str| {
            let buf = buffer(board, 140, 20);
            let list = 140;
            (0..20).find(|&y| {
                let text: String = line(&buf, y).chars().take(list).collect();
                text.contains(&format!("{key} msg"))
            })
        };
        let y = y_of(&board, "r25").expect("r25 is in view with r30 selected");
        board.click(10, y);
        assert_eq!(board.selected, Some(Spot::Row("local/r25".into())));
        assert_eq!(y_of(&board, "r25"), Some(y), "the list kept its place");
        board.click(10, y);
        assert_eq!(
            board.selected,
            Some(Spot::Row("local/r25".into())),
            "a second click, same row"
        );
        board.selected = Some(Spot::Row("local/r02".into()));
        assert!(
            y_of(&board, "r02").is_some(),
            "above the view it scrolls up to the selection"
        );
    }

    #[test]
    fn the_wheel_stops_at_the_ends_and_the_mouse_waits_for_a_prompt() {
        let mut board = table(140);
        buffer(&board, 140, 30);
        let first = board.selected.clone();
        board.scroll(0, true);
        assert_eq!(board.selected, first, "no wrap at the top");
        for _ in 0..10 {
            board.scroll(0, false);
        }
        let last = board.selected.clone();
        board.scroll(0, false);
        assert_eq!(board.selected, last, "no wrap at the bottom");
        assert_ne!(first, last);
        board.filtering = true;
        board.scroll(0, true);
        assert_eq!(board.selected, last, "behind the / prompt");
        board.click(10, 2);
        assert_eq!(board.sort, None);
        board.filtering = false;
        board.commanding = true;
        board.scroll(0, true);
        assert_eq!(board.selected, last, "behind the : prompt");
    }

    #[test]
    fn a_narrow_header_sorted_by_where_never_underflows() {
        for width in 0..=20u16 {
            let columns = Columns::new(width, usize::from(width), &[]);
            for dir in [Dir::Up, Dir::Down] {
                columns.header(Some((Col::Where, dir)));
            }
        }
    }

    #[test]
    fn with_no_rows_a_click_does_nothing() {
        let mut app = App::default();
        app.view = crate::tui::views::views().remove(1);
        (app.width, app.height) = (140, 30);
        app.click(30, 2);
        assert_eq!(app.sort, None);
    }

    #[test]
    fn the_wheel_moves_and_a_click_selects_a_row_in_the_table_view() {
        use crate::tui::app::Spot;
        let mut board = table(140);
        let buf = buffer(&board, 140, 30);
        let y = (3..30)
            .find(|&y| line(&buf, y).contains("seat-a msg"))
            .unwrap();
        board.click(10, y);
        assert_eq!(board.selected, Some(Spot::Row("local/seat-a".into())));
        board.scroll(0, true);
        assert_ne!(
            board.selected,
            Some(Spot::Row("local/seat-a".into())),
            "up moves"
        );
        board.scroll(0, false);
        assert_eq!(
            board.selected,
            Some(Spot::Row("local/seat-a".into())),
            "down comes back"
        );
        board.click(10, 28);
        assert_eq!(
            board.selected,
            Some(Spot::Row("local/seat-a".into())),
            "below the rows nothing changes"
        );
        let mut status = app();
        (status.width, status.height) = (140, 30);
        let before = status.selected.clone();
        buffer(&status, 140, 30);
        status.click(10, y);
        assert_eq!(status.selected, before, "row clicks are table-only");
    }

    #[test]
    fn opening_the_table_view_says_how_to_sort_and_select_once() {
        use crate::tui::app::testkit::press;
        use ratatui::crossterm::event::KeyCode;
        let mut app = app();
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(
            app.toast(),
            Some("click a header to sort · hold Option (iTerm2) or Shift to select text")
        );
        app.toast = None;
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('v'));
        }
        assert_eq!(app.view.name, "table");
        assert_eq!(app.toast(), None, "once per run");
    }

    #[test]
    fn where_and_msg_keep_their_100_column_widths_at_every_board_width() {
        let app = app();
        for width in 100..=240u16 {
            let header = line(&buffer(&app, width, 30), 2);
            let at = |name: &str| header.find(name).unwrap();
            let place = header.find("WHERE").unwrap_or_else(|| at("SESSION"));
            assert!(
                at("MSG") - place >= 15 && header.len() - at("MSG") >= 10,
                "{width}: WHERE, or SESSION to BLOCK, and MSG:\n{header}"
            );
            if header.contains("SESSION") {
                let parts = [at("SESSION"), at("WINDOW"), at("BLOCK"), at("MSG")];
                assert!(
                    parts.windows(2).all(|w| w[1] - w[0] >= 10),
                    "{width}: each part at least 10:\n{header}"
                );
            }
        }
    }

    #[test]
    fn turn_shows_from_140_columns_with_a_dash_for_no_turn() {
        assert!(!line(&buffer(&app(), 139, 30), 2).contains("TURN"));
        let mut app = App::default();
        let mut timed = row("local/quick", "done", "needs_you", false);
        timed.last_turn = Some(125);
        let untimed = row("local/fresh", "blocked", "needs_you", false);
        app.set_rows(vec![untimed, timed], hosts());
        let buf = buffer(&app, 140, 30);
        let x = line(&buf, 2).find("TURN").unwrap();
        let cell = |needle: &str| {
            let y = (3..30).find(|&y| line(&buf, y).contains(needle)).unwrap();
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
            let y = (3..30).find(|&y| line(&buf, y).contains(needle)).unwrap();
            line(&buf, y)
        };
        assert!(row_at(100, "ask msg").starts_with("! permission "));
        assert!(row_at(100, "pick msg").starts_with("? question "));
        assert!(row_at(100, "plain msg").starts_with("! blocked "));
        assert!(row_at(100, "stale msg").starts_with("○ idle? "));
        let narrow = row_at(99, "ask msg");
        assert!(
            narrow.starts_with("! 5h43m "),
            "no STATE below 100: {narrow}"
        );
        assert!(!narrow.contains("permission") && !narrow.contains("blocked"));
    }

    #[test]
    fn where_keeps_v0_1_width_from_100_and_its_three_parts_take_two_thirds_once_split() {
        let at = |board: u16| {
            let columns = Columns::new(board, usize::from(board) - 39, &[]);
            [Col::Where, Col::Session, Col::Window, Col::Block, Col::Msg].map(|c| columns.width(c))
        };
        assert_eq!(at(100), [15, 0, 0, 0, 10], "v0.1's WHERE at 100");
        assert_eq!(at(120), [26, 0, 0, 0, 19], "v0.1's WHERE at 120");
        assert_eq!(at(131), [32, 0, 0, 0, 24], "a 92-column list keeps WHERE");
        assert_eq!(
            at(132),
            [0, 12, 12, 14, 19],
            "a 93-column list splits, no TURN yet"
        );
        assert_eq!(at(140), [0, 12, 12, 14, 20], "a 101-column list adds TURN");
    }

    #[test]
    fn where_splits_from_a_93_column_list_and_turn_joins_at_101() {
        let header = |app: &App, width: u16| line(&buffer(app, width, 30), 2);
        let split = |text: &str| {
            ["SESSION", "WINDOW", "BLOCK"]
                .iter()
                .all(|h| text.contains(h))
                && !text.contains("WHERE")
        };
        let at = |board: &App, width| {
            let text = header(board, width);
            (split(&text), text.contains("TURN"))
        };
        let mut picking = app();
        picking.pick = true;
        for board in [app(), picking] {
            assert_eq!(at(&board, 131), (false, false));
            assert_eq!(at(&board, 132), (true, false), "the list is the board");
            assert_eq!(at(&board, 139), (true, false));
            assert_eq!(at(&board, 140), (true, true));
        }
    }

    fn placed(key: &str, session: &str, tab: &str) -> Row {
        let mut r = row(key, "blocked", "needs_you", false);
        r.pane.session = session.into();
        r.pane.tab = tab.into();
        r
    }

    fn order(app: &App) -> Vec<String> {
        app.items()
            .iter()
            .filter_map(|item| match item {
                crate::tui::app::Item::Row(i) => Some(app.rows[*i].pane.pane.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn e_n_and_b_sort_by_session_window_and_block_and_again_reverse() {
        use crate::tui::app::testkit::sort;

        let mut app = App::default();
        app.set_rows(
            vec![
                placed("local/blk-c", "ses-b", "win-a"),
                placed("local/blk-a", "ses-c", "win-b"),
                placed("local/blk-b", "ses-a", "win-c"),
            ],
            hosts(),
        );
        for (letter, col, up) in [
            ('e', Col::Session, ["blk-b", "blk-c", "blk-a"]),
            ('n', Col::Window, ["blk-c", "blk-a", "blk-b"]),
            ('b', Col::Block, ["blk-a", "blk-b", "blk-c"]),
        ] {
            sort(&mut app, letter);
            assert_eq!(app.sort, Some((col, Dir::Up)), "{letter}");
            assert_eq!(order(&app), up, "{letter}");
            sort(&mut app, letter);
            assert_eq!(app.sort, Some((col, Dir::Down)), "{letter} again");
            let mut down = up;
            down.reverse();
            assert_eq!(order(&app), down, "{letter} again");
        }
    }

    #[test]
    fn the_filter_matches_session_window_and_block() {
        use crate::tui::app::testkit::press;
        use ratatui::crossterm::event::KeyCode;
        for query in ["ses-c", "win-b", "blk-a"] {
            let mut app = App::default();
            app.set_rows(
                vec![
                    placed("local/blk-c", "ses-b", "win-a"),
                    placed("local/blk-a", "ses-c", "win-b"),
                ],
                hosts(),
            );
            press(&mut app, KeyCode::Char('/'));
            for c in query.chars() {
                press(&mut app, KeyCode::Char(c));
            }
            press(&mut app, KeyCode::Enter);
            assert_eq!(order(&app), ["blk-a"], "/{query}");
        }
    }

    /// The rendered lines of the rows whose BLOCK or WHERE ends in `seat`, top to bottom.
    fn seats(rows: Vec<Row>, width: u16) -> Vec<String> {
        let mut app = App::default();
        app.set_rows(rows, hosts());
        let buf = buffer(&app, width, 30);
        (3..30)
            .map(|y| line(&buf, y))
            // The card's lines start with its bar or a space; list rows with their state.
            .filter(|l| l.contains("seat") && !l.starts_with(['━', ' ']))
            .collect()
    }

    fn seat(key: &str, session: &str, window_id: &str) -> Row {
        let mut r = placed(key, session, "main");
        r.pane.pane = "seat".into();
        r.pane.window_id = window_id.into();
        r
    }

    #[test]
    fn same_named_windows_in_one_session_still_number_their_twins() {
        let rows = || {
            vec![
                seat("local/blk-1", "api", "window:a"),
                seat("local/blk-2", "api", "window:b"),
            ]
        };
        for (width, label) in [(200, "SESSION .. BLOCK"), (120, "WHERE")] {
            let lines = seats(rows(), width);
            assert!(
                lines[0].contains("seat #1") && lines[1].contains("seat #2"),
                "{label} at {width}: {lines:#?}"
            );
        }
    }

    #[test]
    fn rows_that_only_read_the_same_once_squeezed_are_numbered() {
        let lines = seats(
            vec![
                seat("local/blk-1", "api-gateway-one-production", "window:a"),
                seat("local/blk-2", "api-gateway-two-production", "window:b"),
            ],
            132,
        );
        assert!(lines[0].contains("…roduction"), "squeezed: {lines:#?}");
        assert!(
            lines[0].contains("seat #1") && lines[1].contains("seat #2"),
            "{lines:#?}"
        );
    }

    #[test]
    fn names_that_only_differ_in_the_middle_are_numbered() {
        let named = |key: &str, block: &str| {
            let mut r = placed(key, "api", "main");
            r.pane.pane = block.into();
            r
        };
        let lines = seats(
            vec![
                named("local/blk-1", "seat-review-agent-one-claude-code"),
                named("local/blk-2", "seat-review-agent-two-claude-code"),
            ],
            132,
        );
        assert_eq!(lines.len(), 2, "{lines:#?}");
        assert!(
            lines[0].contains(" #1 ") && lines[1].contains(" #2 "),
            "{lines:#?}"
        );
    }

    #[test]
    fn the_split_columns_keep_both_ends_and_tell_twins_apart() {
        let mut app = App::default();
        let mut long = placed(
            "local/blk-1",
            "web-frontend-service",
            "2-toolrig:rig:demo-team#0123456789abcdef#l2",
        );
        long.pane.pane = "seat".into();
        let mut twin = long.clone();
        twin.key = "local/blk-2".into();
        twin.pane.block_id = "blk-2".into();
        app.set_rows(vec![long, twin], hosts());
        let buf = buffer(&app, 161, 30);
        let rows: Vec<String> = (3..30)
            .map(|y| line(&buf, y))
            .filter(|l| l.contains("seat") && !l.starts_with(['━', ' ']))
            .collect();
        assert_eq!(rows.len(), 2, "{rows:#?}");
        let columns = Columns::new(161, 161, &[]);
        let session = columns.width(Col::Session) - 1;
        assert!(
            rows[0].contains(&squeeze("web-frontend-service", session)),
            "{rows:#?}"
        );
        assert!(
            rows[0].contains("#l2"),
            "the window keeps its tail: {rows:#?}"
        );
        assert!(
            rows[0].contains("seat #1") && rows[1].contains("seat #2"),
            "{rows:#?}"
        );
    }

    #[test]
    fn under_12_rows_the_header_and_its_gap_give_their_lines_back_to_the_list() {
        let buf = buffer(&app(), 80, 12);
        assert_eq!(line(&buf, 1).trim(), "", "a blank row under the top bar");
        assert!(line(&buf, 2).contains("AGE"));
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
        let columns = Columns::new(120, 81, &[]);
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
                Some('H'),
                Some('O'),
                Some('W'),
                Some('M')
            ],
            "every column's letter"
        );
    }
}
