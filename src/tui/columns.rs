//! The list's columns: one table says which columns a board width shows, how wide each is and
//! what goes in it, for the rows and their header alike.

use std::cmp::Ordering;
use std::ops::RangeInclusive;

use chrono::{DateTime, Utc};

use super::theme;
use crate::board::Row;
use crate::output::{
    clean, fit, harness, in_state, labels, name, path, span, squeeze, state_word, what,
};

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Tier {
    /// Under 60 columns: glyph, time in state, session › pane, message.
    Narrow,
    /// 60-99: adds harness and host.
    Mid,
    /// 100 and up: adds the state (or its kind) and the tab, with the detail pane on the right.
    /// From 132 (a 93-column list beside the default pane at 140) WHERE splits into SESSION,
    /// WINDOW and BLOCK; TURN joins at 140.
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
    /// A share of what the fixed columns leave: WHERE's, one of its three parts, or MSG's.
    Where,
    Part(usize),
    Msg,
}

/// The board width from which WHERE splits into SESSION, WINDOW and BLOCK: 140 with the default
/// third leaves a 93-column list, which `columns_board` counts as 132.
pub const SPLIT: u16 = 132;
const ALWAYS: RangeInclusive<u16> = 0..=u16::MAX;

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

/// Every column, left to right. Sort letters follow k9s: S state, A age, T turn, R harness
/// (runtime), O host, W where, E session, N window, B block, M msg.
pub const COLUMNS: [Column; 11] = [
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
        Some('S'),
        Dir::Up,
        100..=u16::MAX,
        Width::Fixed(11),
        Tone::State,
    ),
    column(
        Col::Age,
        "AGE",
        Some('A'),
        Dir::Down,
        ALWAYS,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Turn,
        "TURN",
        Some('T'),
        Dir::Down,
        140..=u16::MAX,
        Width::Fixed(7),
        Tone::Soft,
    ),
    column(
        Col::Harness,
        "HARNESS",
        Some('R'),
        Dir::Up,
        60..=u16::MAX,
        Width::Fixed(8),
        Tone::Soft,
    ),
    column(
        Col::Host,
        "HOST",
        Some('O'),
        Dir::Up,
        60..=u16::MAX,
        Width::Fixed(8),
        Tone::Dim,
    ),
    column(
        Col::Where,
        "WHERE",
        Some('W'),
        Dir::Up,
        0..=SPLIT - 1,
        Width::Where,
        Tone::Soft,
    ),
    column(
        Col::Session,
        "SESSION",
        Some('E'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(0),
        Tone::Soft,
    ),
    column(
        Col::Window,
        "WINDOW",
        Some('N'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(1),
        Tone::Soft,
    ),
    column(
        Col::Block,
        "BLOCK",
        Some('B'),
        Dir::Up,
        SPLIT..=u16::MAX,
        Width::Part(2),
        Tone::Soft,
    ),
    column(
        Col::Msg,
        "MSG",
        Some('M'),
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
        Col::Session => text(&row.pane.session),
        Col::Window => text(&row.pane.tab),
        Col::Block => text(&name(row)),
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
        let shown: Vec<&Column> = COLUMNS
            .iter()
            .filter(|c| c.boards.contains(&board))
            .collect();
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
                    Col::Glyph => theme::glyph(row).to_owned(),
                    Col::State => state(row),
                    Col::Age => in_state(row, now),
                    Col::Turn => row.last_turn.map_or_else(|| "-".to_owned(), span),
                    Col::Harness => harness(row),
                    Col::Host => row.pane.host.clone(),
                    Col::Where => place.to_owned(),
                    Col::Session => squeeze(&clean(&row.pane.session), width.saturating_sub(1)),
                    Col::Window => squeeze(&clean(&row.pane.tab), width.saturating_sub(1)),
                    Col::Block => squeeze(place, width.saturating_sub(1)),
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
                    .filter(|_| text.starts_with(c.header));
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
            usize::from(width) - usize::from(crate::tui::detail::width(width, app.detail)) - 1
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
            let header: String = line(&buf, 2).chars().take(list).collect();
            let columns = Columns::new(
                crate::tui::detail::columns_board(width, list, app.pick),
                list,
            );
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
    fn where_and_msg_keep_their_100_column_widths_at_any_pane_width() {
        let mut app = app();
        for width in 100..=240u16 {
            for share in [None, Some(0.0), Some(0.3), Some(0.4), Some(0.5), Some(0.9)] {
                app.detail = share;
                let header: String = line(&buffer(&app, width, 30), 2)
                    .chars()
                    .take(list_width(&app, width))
                    .collect();
                let at = |name: &str| header.find(name).unwrap();
                let place = header.find("WHERE").unwrap_or_else(|| at("SESSION"));
                assert!(
                    at("MSG") - place >= 15 && header.len() - at("MSG") >= 10,
                    "{width} at {share:?}: WHERE, or SESSION to BLOCK, and MSG:\n{header}"
                );
                if header.contains("SESSION") {
                    let parts = [at("SESSION"), at("WINDOW"), at("BLOCK"), at("MSG")];
                    assert!(
                        parts.windows(2).all(|w| w[1] - w[0] >= 10),
                        "{width} at {share:?}: each part at least 10:\n{header}"
                    );
                }
            }
        }
    }

    #[test]
    fn turn_shows_once_the_list_has_room_with_a_dash_for_no_turn() {
        let mut narrow = app();
        narrow.detail = Some(38.0 / 139.0);
        assert!(!line(&buffer(&narrow, 139, 30), 2).contains("TURN"));
        assert!(
            !line(&buffer(&app(), 140, 30), 2).contains("TURN"),
            "a 93-column list beside the default 46"
        );
        let mut app = App::default();
        app.detail = Some(38.0 / 140.0);
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
    fn where_keeps_v0_1_width_from_100_and_its_three_parts_take_two_thirds_once_split() {
        let at = |board: u16| {
            let columns = Columns::new(board, usize::from(board) - 39);
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
        let mut board = app();
        let at = |board: &App, width| {
            let text = header(board, width);
            (split(&text), text.contains("TURN"))
        };
        assert_eq!(
            at(&board, 139),
            (false, false),
            "the default 46 pane: a 92-column list"
        );
        assert_eq!(at(&board, 140), (true, false), "the default 46 pane: 93");
        assert_eq!(at(&board, 151), (true, false), "the default 50 pane: 100");
        assert_eq!(at(&board, 152), (true, true), "the default 50 pane: 101");
        board.detail = Some(38.0 / 140.0);
        assert_eq!(at(&board, 140), (true, true), "a 38 pane: 101");
        let mut picking = app();
        picking.pick = true;
        assert_eq!(at(&picking, 131), (false, false));
        assert_eq!(
            at(&picking, 132),
            (true, false),
            "pick splits at its own width"
        );
        assert_eq!(at(&picking, 140), (true, true));
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
        use crate::tui::app::tests::press;
        use ratatui::crossterm::event::KeyCode;
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
            ('E', Col::Session, ["blk-b", "blk-c", "blk-a"]),
            ('N', Col::Window, ["blk-c", "blk-a", "blk-b"]),
            ('B', Col::Block, ["blk-a", "blk-b", "blk-c"]),
        ] {
            press(&mut app, KeyCode::Char(letter));
            assert_eq!(app.sort, Some((col, Dir::Up)), "{letter}");
            assert_eq!(order(&app), up, "{letter}");
            press(&mut app, KeyCode::Char(letter));
            assert_eq!(app.sort, Some((col, Dir::Down)), "{letter} again");
            let mut down = up;
            down.reverse();
            assert_eq!(order(&app), down, "{letter} again");
        }
    }

    #[test]
    fn the_filter_matches_session_window_and_block() {
        use crate::tui::app::tests::press;
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
    fn seats(rows: Vec<Row>, width: u16, detail: Option<f64>) -> Vec<String> {
        let mut app = App::default();
        app.set_rows(rows, hosts());
        app.detail = detail;
        let buf = buffer(&app, width, 30);
        (3..30)
            .map(|y| line(&buf, y))
            .filter(|l| l.contains("seat"))
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
            let lines = seats(rows(), width, None);
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
                seat("local/blk-1", "api-gateway-one-prod", "window:a"),
                seat("local/blk-2", "api-gateway-two-prod", "window:b"),
            ],
            140,
            None,
        );
        assert!(lines[0].contains("api-g…-prod"), "{lines:#?}");
        assert!(
            lines[0].contains("seat #1") && lines[1].contains("seat #2"),
            "{lines:#?}"
        );
    }

    #[test]
    fn the_split_columns_keep_both_ends_and_tell_twins_apart() {
        let mut app = App::default();
        let mut long = placed(
            "local/blk-1",
            "web-frontend-service",
            "2-openrig:rig:demo-team#0123456789abcdef#l2",
        );
        long.pane.pane = "seat".into();
        let mut twin = long.clone();
        twin.key = "local/blk-2".into();
        twin.pane.block_id = "blk-2".into();
        app.set_rows(vec![long, twin], hosts());
        app.detail = Some(38.0 / 200.0);
        let buf = buffer(&app, 200, 30);
        let rows: Vec<String> = (3..30)
            .map(|y| line(&buf, y))
            .filter(|l| l.contains("seat"))
            .collect();
        assert_eq!(rows.len(), 2, "{rows:#?}");
        let columns = Columns::new(200, 161);
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
                Some('W'),
                Some('M')
            ],
            "every column's letter"
        );
    }
}
