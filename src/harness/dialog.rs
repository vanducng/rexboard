//! The literals a harness's screen is read by: how it marks a dialog's highlighted option and its
//! input line, and which option labels grant something for later. The board's parser and
//! `send.lua`'s re-check both read this one copy, so they cannot drift apart.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DialogSyntax {
    /// Before a dialog's highlighted option: `❯ 1. Yes`.
    pub cursor: &'static [char],
    /// Before an input line: `❯ text`, or `│ > text │` in a box.
    pub input: &'static [char],
    /// After an option's digit: `1. Yes`, `2) No`.
    pub separators: &'static [char],
    /// Words in a label, lowercase, that make the option last past this dialog.
    pub lasting: &'static [&'static str],
}

/// The grammar every built-in harness shares today.
pub const SHARED: DialogSyntax = DialogSyntax {
    cursor: &['❯', '>'],
    input: &['❯', '›', '>'],
    separators: &['.', ')'],
    lasting: &[
        "don't ask again",
        "always",
        "auto mode",
        "this session",
        "accept edits",
    ],
};
