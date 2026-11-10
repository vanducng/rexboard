# 18. Rows can be marked

**Status:** Accepted, 2026-11-10

## Context

On a board with many panes, a few matter more to one user than the rest: the pane they answer from, the seat that coordinates a team. Today nothing on a row says so, and finding it means reading every `WHERE`. [15](0015-scope-and-non-goals.md) refuses user-defined columns, views and layouts, so the answer cannot be a custom view or a pinned group. A tool around the agents may also know which pane is the one to watch, and must say so without rexboard naming the tool ([4](0004-extensions-not-a-built-in-core.md)).

## Decision

- **A mark is one style, nothing more.** A marked row keeps its group, its place and its columns. It shows `*` before its `#` number, which reads without colour in every font ([principle 4](README.md#principles)); the `#` column is 4 cells at every width, so a mark never shifts a column. [8](0008-one-style-system.md) gains one role, `marked` (cyan, bold, the same look as `accent`: this row matters to you). The `*` and the row's location cells (`WHERE`, or `SESSION`, `WINDOW` and `BLOCK` once it is split) take it; the number stays dim. The selected row's raised background still wins. `▸` is not used: it already marks a folded group.
- **Three sources, one mark.** A row is marked when any of these says so:
  - a `[[mark]]` rule in `config.toml`, matching `session`, `window`, `block`, `host` or `field.<id>`, each an exact value or a glob (`*`, `?`); every key of one rule must match;
  - an extension's answer, with the optional `"mark": true` on a row (contract 1, additive: an older rexboard drops the key and logs it once);
  - the user's own `*` on the board, kept in `favorites.json` in the state dir, beside `pick.json`, for [17](0017-pick-keeps-its-view-and-sort-in-its-own-file.md)'s reason.
- **serve decides.** serve applies the rules and the extensions' marks, so the board, `pick`, `ls` and `next` agree. `ls --json` carries `marked` (bool) and `marked_by` (the sources, deduped: `config`, an extension id, `favorite`). A mark is data about the row, the result of the user's own rules, not presentation ([principle 2](README.md#principles)).
- **Reaching a marked row:** `'` on the board and in `pick` goes to the first marked row in list order and, pressed again, to the next; `rexboard next --marked` does it from a script. The footer shows `' marked` only while a marked row exists and where it fits (from about 85 columns); `?` always lists it.
- **Fails open.** A rule that does not parse is skipped and named once in `serve.log` and `rexboard ext ls`-style output; a missing, unreadable or malformed `favorites.json` means no favorites, one message, no retry.

## Consequences

- One config table, one optional extension key, one style role, one state file and one key, each named in its own page.
- A broad glob can mark half the board. `rexboard ls --marked` shows what the rules catch.
- Favorites never reorder rows. A user who wants a pane first uses a sort or a filter.

## Supersedes

Narrows [15](0015-scope-and-non-goals.md): "user-defined columns, views or layouts" stays out; a mark changes one style and adds none of those. Amends [4](0004-extensions-not-a-built-in-core.md) (one optional response key) and [8](0008-one-style-system.md) (one role).
