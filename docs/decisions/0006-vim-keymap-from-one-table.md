# 6. A vim keymap from one table

**Status:** Accepted, 2026-10-27

## Context

The operator lives in vim and in a stock Rex window, and wants the board's keys to feel native in both. Rex binds cmd chords and passes every other key to the pane, so plain keys and ctrl chords reach the board unless something else binds them. The first keymap grew one key at a time; its help text and footers could drift from what the keys did.

## Decision

- Plain keys first, vim's meaning where vim has one: `j`/`k`, `gg`/`G`, `H`/`M`/`L`, `n`/`N`, `]`/`[`, `za zo zc zR zM`, `dd`, `yy`, `:q`, `-` and `ctrl-o`.
- The only ctrl chords are vim's own: `ctrl-d`, `u`, `f`, `b`, `e`, `y`, `o` and `c`. No alt or cmd chords, no `ctrl-s`, no `ctrl-i` or `ctrl-m` (they are tab and enter).
- One table, `src/tui/keys.rs`, drives what each key does, the `?` table and every footer. Tests check that no sequence is bound twice and every key does something.
- A prefix key (`g`, `o`, `z`, `y`, `f`, `d`) shows what may follow it in the footer; `esc` cancels it.
- Sorting is `o` and a lowercase column letter. Capital letters are not sort keys.
- Bulk actions that cannot be undone are not on a single key: marking every done row seen is `:seen`.
- `q` closes the layer in front, and quits only from the bare board.
- When a key moves, the old key says where it went for one release.

## Consequences

- A key cannot say one thing in `?` and do another.
- Adding a key means one entry in `keys.rs`, and its help and footer come with it.
- Every chord is checked against stock Rex with a raw-byte probe before a release.

## Supersedes

The k9s-style capital sort letters (`S A T R O W E N B M`) and `ctrl+s` for bulk seen.
