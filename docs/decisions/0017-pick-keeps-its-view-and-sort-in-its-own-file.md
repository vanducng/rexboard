# 17. `pick` keeps its view and sort in its own file

**Status:** Accepted, 2026-11-01

## Context

`rexboard pick` starts fresh on every run, in a pane that lives for one choice, so a user who always picks from the same view and sort sets them again each time. The board's own settings are in `tui.json`, which the board rewrites whole from what it holds whenever `c` is pressed. A `pick` writing its keys there would race that rewrite, and either side could wipe the other's keys. Several `pick` panes can also be open at once.

## Decision

- **A separate file:** `pick.json`, in the state dir beside `tui.json`, holds `{"view", "sort"}` for `pick` only. The board never reads it, so the board's starting view and sort are unchanged.
- **Only what the user changed:** a save is triggered by a view or sort change, never by a filter or the `f` status. A view or sort that is not available yet (an extension's field) stays as read until it is, so a temporary default does not overwrite it. A sort on a column that is hidden is not applied.
- **Safe to write:** the file is written whole through a temp file and a rename, and keys it does not know are kept. A missing, unreadable or malformed file means the defaults.
- **Fails loudly once:** a failed save says so in the footer, as a failed `tui.json` save does, and is not retried until the next change.

## Consequences

- One more file in the state dir, named in [system architecture](../system-architecture.md#files), and one more way to reset: delete it.
- A `pick` and the board never write the same file, so neither needs a lock.
- The last `pick` to change a setting wins; two panes do not merge their changes.

## Supersedes

None. It follows [principle 1](README.md#principles): the setting could not live in `tui.json` without the race above.
