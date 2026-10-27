# 8. One style system

**Status:** Accepted, 2026-10-27. Amended 2026-10-27

## Context

The board shows about 40 rows that must be read at a glance, in terminals with different themes, fonts and widths, sometimes with colour hard to tell apart. Styles had grown per screen, and a working row could look idle.

## Decision

- State reads by shape first, colour second: `!` blocked, `?` question, `✗` error, `✓` done (bold green unseen, dim seen), a braille spinner for working, `○` idle, `·` unknown, `◌` down. Every glyph is one cell wide and never an emoji.
- ANSI named colours only for the board's own text, so the terminal's theme carries through. (The full view shows a pane's own colours as the pane drew them.)
- Only working moves, on rexboard's own clock, and the board redraws fast only while a working row is on screen. Reduced motion (`tui.json` or `REXBOARD_REDUCED_MOTION=1`) holds it still.
- One module, `src/tui/theme.rs`, holds the state table and a few text roles (`alert`, `note`, `accent`, `strong`, `quiet`, `header`, `selected`, the cut mark). Nothing outside `theme.rs` and `icons.rs` picks a colour or a glyph, and a test checks it.
- A glyph keeps its state's look everywhere, so a bar never draws the spinner bold.
- Icons are opt-in Nerd Font marks (`"icons": "nerd"`, Nerd Fonts 3.5 or newer), all in `src/tui/icons.rs`. Without them the board shows the words they stand for. A harness's mark is declared on its adapter, not in a second list.
- An agent's markdown maps only to the text roles above: emphasis to `strong`, code plain or `strong`, never dimmed. It never takes a state's colour or glyph. The CLI and what `yy` copies keep the raw text.

## Consequences

- A new screen picks a role, not a colour, and stays consistent.
- A change of look shows as a diff in the style snapshot test.
- Not built yet: harness marks on the adapter (today `icons::harness()` is a second list), and block markdown (lists, headings, tables, code blocks); the board draws inline markdown only.

## Supersedes

Per-screen colours, and `●` and `◐` for state (wide in some terminals), and the emoji-capable `✖`. Amended 2026-10-27: icons and markdown.
