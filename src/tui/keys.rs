//! The keymap: one table per layer that the key handling, the `?` table and the footers all
//! read, so a key cannot say one thing and do another. Plain keys and vim's own ctrl chords
//! only: Rex passes an unbound key to the pane, and its defaults are all cmd chords.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Down,
    Up,
    First,
    Last,
    Top,
    Middle,
    Bottom,
    HalfDown,
    HalfUp,
    PageDown,
    PageUp,
    NextGroup,
    PrevGroup,
    NextNeeds,
    PrevNeeds,
    /// `1`-`9`, then maybe another: the row with that number on screen.
    Nth,
    Jump,
    Back,
    Filter,
    /// `f`, then a status letter.
    Status,
    Command,
    NextView,
    PrevView,
    /// `o`, then a column's letter.
    Sort,
    /// `x`, then an extension action's key.
    Actions,
    FoldToggle,
    FoldOpen,
    FoldClose,
    OpenAll,
    FoldAll,
    Card,
    /// `p`: the live preview strip.
    Preview,
    CardDown,
    CardUp,
    Full,
    Answer,
    Seen,
    Close,
    CopyMsg,
    CopyPlace,
    Reconnect,
    Keys,
    Cancel,
    Quit,
    /// The full view's way back to the board.
    Leave,
    /// An old key: says where it went and does nothing else. Gone in the release after next.
    Moved(&'static str),
}

/// A key as the table names it: its code, and whether ctrl is held. Shift is in the character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K(pub KeyCode, pub bool);

impl K {
    /// None for alt, cmd and other chords, which the board never binds.
    pub fn of(key: KeyEvent) -> Option<K> {
        let others = key.modifiers - KeyModifiers::SHIFT - KeyModifiers::CONTROL;
        others
            .is_empty()
            .then(|| K(key.code, key.modifiers.contains(KeyModifiers::CONTROL)))
    }

    pub fn char(self) -> Option<char> {
        match self {
            K(KeyCode::Char(c), false) => Some(c),
            _ => None,
        }
    }
}

const fn c(ch: char) -> K {
    K(KeyCode::Char(ch), false)
}

const fn ctrl(ch: char) -> K {
    K(KeyCode::Char(ch), true)
}

const fn code(code: KeyCode) -> K {
    K(code, false)
}

pub struct Entry {
    pub act: Act,
    /// Every key sequence that does it: one key, or a prefix and a key.
    pub seqs: &'static [&'static [K]],
    /// How `?` lists the keys; empty when another row lists them, or for a moved key.
    pub keys: &'static str,
    pub help: &'static str,
    /// The footer's word for it, and a prefix's word for its second key.
    pub word: &'static str,
}

const fn e(
    act: Act,
    seqs: &'static [&'static [K]],
    keys: &'static str,
    help: &'static str,
    word: &'static str,
) -> Entry {
    Entry {
        act,
        seqs,
        keys,
        help,
        word,
    }
}

/// The board, in the order `?` lists it.
pub const BOARD: &[Entry] = &[
    e(
        Act::Down,
        &[&[c('j')], &[code(KeyCode::Down)]],
        "j/k",
        "move; the ends wrap",
        "move",
    ),
    e(Act::Up, &[&[c('k')], &[code(KeyCode::Up)]], "", "", "up"),
    e(
        Act::First,
        &[&[c('g'), c('g')], &[code(KeyCode::Home)]],
        "gg G",
        "first, last row",
        "top",
    ),
    e(
        Act::Last,
        &[&[c('G')], &[code(KeyCode::End)]],
        "",
        "",
        "last",
    ),
    e(
        Act::Top,
        &[&[c('H')]],
        "H M L",
        "screen top, middle, bottom",
        "top",
    ),
    e(Act::Middle, &[&[c('M')]], "", "", "middle"),
    e(Act::Bottom, &[&[c('L')]], "", "", "bottom"),
    e(
        Act::HalfDown,
        &[&[ctrl('d')]],
        "ctrl-d/u",
        "half a page down, up",
        "half page",
    ),
    e(Act::HalfUp, &[&[ctrl('u')]], "", "", "half page up"),
    e(
        Act::PageDown,
        &[&[ctrl('f')]],
        "ctrl-f/b",
        "a page down, up",
        "page",
    ),
    e(Act::PageUp, &[&[ctrl('b')]], "", "", "page up"),
    e(
        Act::NextGroup,
        &[&[c(']')]],
        "] [",
        "next, previous group",
        "group",
    ),
    e(Act::PrevGroup, &[&[c('[')]], "", "", "previous group"),
    e(
        Act::NextNeeds,
        &[&[c('n')], &[code(KeyCode::Tab)]],
        "n N tab",
        "next, previous needing you",
        "needs you",
    ),
    e(Act::PrevNeeds, &[&[c('N')]], "", "", "previous"),
    e(
        Act::Nth,
        &[
            &[c('1')],
            &[c('2')],
            &[c('3')],
            &[c('4')],
            &[c('5')],
            &[c('6')],
            &[c('7')],
            &[c('8')],
            &[c('9')],
        ],
        "1-99",
        "row by number; 2 digits wait",
        "select",
    ),
    e(
        Act::Jump,
        &[&[code(KeyCode::Enter)]],
        "enter",
        "go to the pane",
        "jump",
    ),
    e(
        Act::Back,
        &[&[c('-')], &[ctrl('o')]],
        "- ctrl-o",
        "back and forth, like cd -",
        "back",
    ),
    e(
        Act::Filter,
        &[&[c('/')]],
        "/",
        "filter: text or key:value",
        "filter",
    ),
    e(
        Act::Status,
        &[&[c('f')]],
        "f",
        "one status; ff clears",
        "status",
    ),
    e(
        Act::Command,
        &[&[c(':')]],
        ":",
        "a view, :seen or :q",
        "command",
    ),
    e(
        Act::NextView,
        &[&[c('v')], &[c('g'), c('t')]],
        "v V",
        "next, previous view (gt gT)",
        "next view",
    ),
    e(
        Act::PrevView,
        &[&[c('V')], &[c('g'), c('T')]],
        "",
        "",
        "previous view",
    ),
    e(
        Act::Sort,
        &[&[c('o')]],
        "o",
        "sort by a column's letter",
        "sort",
    ),
    e(
        Act::Actions,
        &[&[c('x')]],
        "x",
        "extension actions, e.g. send",
        "action",
    ),
    e(
        Act::FoldToggle,
        &[&[c('z'), c('a')]],
        "za zo zc",
        "fold: toggle, open, close",
        "toggle",
    ),
    e(Act::FoldOpen, &[&[c('z'), c('o')]], "", "", "open"),
    e(Act::FoldClose, &[&[c('z'), c('c')]], "", "", "fold"),
    e(
        Act::OpenAll,
        &[&[c('z'), c('R')]],
        "zR zM",
        "open all, fold all",
        "open all",
    ),
    e(Act::FoldAll, &[&[c('z'), c('M')]], "", "", "fold all"),
    e(
        Act::Preview,
        &[&[c('p')]],
        "p",
        "show or hide the preview strip",
        "preview",
    ),
    e(
        Act::Card,
        &[&[c('c')]],
        "c",
        "show or hide the card",
        "card",
    ),
    e(
        Act::CardDown,
        &[&[ctrl('e')]],
        "ctrl-e/y",
        "scroll the card down, up",
        "scroll card",
    ),
    e(Act::CardUp, &[&[ctrl('y')]], "", "", "card up"),
    e(
        Act::Full,
        &[&[c(' ')]],
        "space",
        "full view of the pane, live",
        "full",
    ),
    e(
        Act::Answer,
        &[&[c('a')], &[c('i')]],
        "a i",
        "answer: pick an option, reply",
        "answer",
    ),
    e(Act::Seen, &[&[c('m')]], "m", "mark seen", "seen"),
    e(
        Act::Close,
        &[&[c('d'), c('d')]],
        "dd",
        "close the pane; y confirms",
        "close",
    ),
    e(
        Act::CopyMsg,
        &[&[c('y'), c('y')]],
        "yy yp",
        "copy the message, the place",
        "message",
    ),
    e(Act::CopyPlace, &[&[c('y'), c('p')]], "", "", "place"),
    e(
        Act::Reconnect,
        &[&[c('R')]],
        "R",
        "reconnect to serve",
        "reconnect",
    ),
    e(
        Act::Keys,
        &[&[c('?')]],
        "?",
        "this table; q or esc closes it",
        "keys",
    ),
    e(
        Act::Cancel,
        &[&[code(KeyCode::Esc)]],
        "esc",
        "cancel a key, clear filters",
        "clear",
    ),
    e(
        Act::Quit,
        &[&[c('q')], &[ctrl('c')]],
        "q",
        "close a layer, else quit",
        "quit",
    ),
    e(Act::Moved("mark seen is m now"), &[&[c('s')]], "", "", ""),
    e(
        Act::Moved("mark every done row seen is :seen now"),
        &[&[ctrl('s')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by state is os now"),
        &[&[c('S')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by age is oa now"),
        &[&[c('A')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by turn is ot now"),
        &[&[c('T')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by host is oo now"),
        &[&[c('O')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by where is ow now"),
        &[&[c('W')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by session is oe now"),
        &[&[c('E')]],
        "",
        "",
        "",
    ),
    e(
        Act::Moved("sort by block is ob now"),
        &[&[c('B')]],
        "",
        "",
        "",
    ),
    e(Act::Moved("reconnect is R now"), &[&[c('r')]], "", "", ""),
    e(
        Act::Moved("copy the place is yp now"),
        &[&[c('Y')]],
        "",
        "",
        "",
    ),
    e(Act::Moved("close is dd now"), &[&[ctrl('k')]], "", "", ""),
];

/// The board's footer, most needed first; a narrow board drops them from the end.
pub const FOOTER: &[Act] = &[
    Act::Down,
    Act::Jump,
    Act::Full,
    Act::Preview,
    Act::Keys,
    Act::NextNeeds,
    Act::Filter,
    Act::Seen,
    Act::Answer,
    Act::Card,
    Act::NextView,
    Act::Nth,
    Act::CardDown,
];

/// The full view, in the order its footer lists it.
pub const FULL: &[Entry] = &[
    e(
        Act::Leave,
        &[&[c(' ')], &[code(KeyCode::Esc)], &[c('q')]],
        "space",
        "back to the board",
        "back",
    ),
    e(
        Act::Down,
        &[&[c('j')], &[code(KeyCode::Down)], &[ctrl('e')]],
        "j/k",
        "scroll a line",
        "scroll",
    ),
    e(
        Act::Up,
        &[&[c('k')], &[code(KeyCode::Up)], &[ctrl('y')]],
        "",
        "",
        "up",
    ),
    e(
        Act::Answer,
        &[&[c('a')], &[c('i')]],
        "a",
        "answer",
        "answer",
    ),
    e(
        Act::Jump,
        &[&[code(KeyCode::Enter)]],
        "enter",
        "go to the pane",
        "jump",
    ),
    e(
        Act::First,
        &[&[c('g'), c('g')], &[code(KeyCode::Home)]],
        "gg/G",
        "top, bottom",
        "top, bottom",
    ),
    e(
        Act::Last,
        &[&[c('G')], &[code(KeyCode::End)]],
        "",
        "",
        "bottom",
    ),
    e(
        Act::HalfDown,
        &[&[ctrl('d')]],
        "ctrl-d/u",
        "half a page",
        "half page",
    ),
    e(Act::HalfUp, &[&[ctrl('u')]], "", "", "half page up"),
    e(
        Act::PageDown,
        &[&[ctrl('f')], &[code(KeyCode::PageDown)]],
        "ctrl-f/b",
        "a page",
        "page",
    ),
    e(
        Act::PageUp,
        &[&[ctrl('b')], &[code(KeyCode::PageUp)]],
        "",
        "",
        "page up",
    ),
    e(Act::Quit, &[&[ctrl('c')]], "", "", "quit"),
];

/// What `seq` does in `table`.
pub fn find(table: &[Entry], seq: &[K]) -> Option<Act> {
    table
        .iter()
        .find(|entry| entry.seqs.contains(&seq))
        .map(|entry| entry.act)
}

/// Whether `key` starts a two-key sequence in `table`.
pub fn starts(table: &[Entry], key: K) -> bool {
    table
        .iter()
        .flat_map(|entry| entry.seqs.iter())
        .any(|seq| seq.len() == 2 && seq[0] == key)
}

/// A prefix's continuations: each second key and its word, in table order.
pub fn after(table: &[Entry], prefix: K) -> Vec<(char, &'static str)> {
    table
        .iter()
        .flat_map(|entry| entry.seqs.iter().map(move |seq| (seq, entry.word)))
        .filter(|(seq, _)| seq.len() == 2 && seq[0] == prefix)
        .filter_map(|(seq, word)| Some((seq[1].char()?, word)))
        .collect()
}

/// The entry for `act` in `table`, as the footer and `?` show it.
pub fn entry(table: &[Entry], act: Act) -> Option<&Entry> {
    table.iter().find(|entry| entry.act == act)
}

/// The key a footer shows for an entry: the first one `?` lists.
pub fn short(entry: &Entry) -> &'static str {
    entry.keys.split_whitespace().next().unwrap_or(entry.keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sequence as docs/tui.md writes it (`gg`, `ctrl-d`, `space`), or None for the keys it
    /// names in words (arrows, Home, End, the page keys).
    fn doc_name(seq: &[K]) -> Option<String> {
        seq.iter()
            .map(|key| match key {
                K(KeyCode::Char(' '), false) => Some("space".to_owned()),
                K(KeyCode::Char(c), false) => Some(c.to_string()),
                K(KeyCode::Char(c), true) => Some(format!("ctrl-{c}")),
                K(KeyCode::Enter, _) => Some("enter".to_owned()),
                K(KeyCode::Esc, _) => Some("esc".to_owned()),
                K(KeyCode::Tab, _) => Some("tab".to_owned()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|names| names.concat())
    }

    /// docs/tui.md names every key the board and the full view bind, each in its own section,
    /// and the Keys table lists no key the board does not bind, such as a moved one.
    #[test]
    fn the_tui_doc_names_every_bound_key_and_no_other() {
        let doc = include_str!("../../docs/tui.md");
        let section = |title: &str| {
            let start = doc.find(title).expect(title) + title.len();
            let rest = &doc[start..];
            &rest[..rest.find("\n## ").unwrap_or(rest.len())]
        };
        let live = |table: &'static [Entry]| {
            table
                .iter()
                .filter(|entry| !matches!(entry.act, Act::Moved(_)))
                .flat_map(|entry| entry.seqs.iter().copied())
        };
        for (table, title) in [(BOARD, "## Keys"), (FULL, "## Full view")] {
            let text = section(title);
            for seq in live(table) {
                let Some(name) = doc_name(seq) else { continue };
                // `1`-`9` names the digits between.
                let digit = name.len() == 1 && name.as_bytes()[0].is_ascii_digit();
                assert!(
                    text.contains(&format!("`{name}`")) || digit && text.contains("`1`-`9`"),
                    "{title} in docs/tui.md does not name `{name}`"
                );
            }
        }
        let bound: std::collections::BTreeSet<String> = live(BOARD)
            .flat_map(|seq| [doc_name(seq), doc_name(&seq[..1])])
            .flatten()
            .collect();
        for row in section("## Keys").lines().filter(|l| l.starts_with("| `")) {
            let cell = row.split('|').nth(1).unwrap_or_default();
            for name in cell.split('`').skip(1).step_by(2) {
                assert!(
                    bound.contains(name),
                    "docs/tui.md lists `{name}`, which the board does not bind: {row}"
                );
            }
        }
    }

    /// No key or sequence means two things in one layer, and the only chords that act are vim's
    /// own; an old chord only says where it went.
    #[test]
    fn every_key_is_bound_once_and_no_chord_is_off_the_list() {
        for table in [BOARD, FULL] {
            let seqs: Vec<(&[K], Act)> = table
                .iter()
                .flat_map(|e| e.seqs.iter().map(|seq| (*seq, e.act)))
                .collect();
            for (i, (seq, act)) in seqs.iter().enumerate() {
                assert!(
                    !seqs[i + 1..].iter().any(|(other, _)| other == seq),
                    "{seq:?} is bound twice"
                );
                // A prefix key is never an action of its own.
                assert!(
                    seq.len() == 2 || !starts(table, seq[0]),
                    "{seq:?} is both a key and a prefix"
                );
                if matches!(act, Act::Moved(_)) {
                    continue;
                }
                for key in seq.iter().filter(|k| k.1) {
                    let K(KeyCode::Char(ch), _) = key else {
                        panic!("{key:?}");
                    };
                    assert!("cdufbeyo".contains(*ch), "ctrl-{ch} is not vim's");
                }
            }
        }
    }

    #[test]
    fn a_prefix_lists_what_follows_it() {
        let words = |p| {
            after(BOARD, c(p))
                .into_iter()
                .map(|(k, w)| format!("{k} {w}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        assert_eq!(words('g'), "g top · t next view · T previous view");
        assert_eq!(
            words('z'),
            "a toggle · o open · c fold · R open all · M fold all"
        );
        assert_eq!(words('y'), "y message · p place");
        assert_eq!(words('d'), "d close");
    }
}
