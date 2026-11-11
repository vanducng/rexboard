# 19. The board opens where it was left

**Status:** Accepted, 2026-11-11

## Context

The board and `board --once` start fresh on every run: the view, the sort, the `f` status and the `/` text last for the run, and only `pick` keeps a view and sort ([17](0017-pick-keeps-its-view-and-sort-in-its-own-file.md)). A user who always works from one view, or narrows to one status, sets it again at every start. The popup over ssh and the board on the desk share one state dir, so a setting made in one should be there in the other. The reason for [17](0017-pick-keeps-its-view-and-sort-in-its-own-file.md)'s separate file holds: `c` rewrites `tui.json` whole from what the board holds, so a second writer there could wipe keys.

## Decision

- **A separate file:** `board.json`, in the state dir beside `tui.json` and `pick.json`, holds `view`, `sort`, `status` and `filter` for the board and `board --once`. `pick` keeps `pick.json` and does not read it: a `pick` is a fresh list for one choice, and the status and text of a board would hide the rows it was opened to choose from.
- **Only what the user changed:** a save follows a change to one of the four, a clear (`esc`, `ff`, a sort cycled off) included, and removes the key. Typing in `/` saves when it ends. A launch flag wins for the run and is not saved.
- **Only the changed keys are written:** the file is read again, those keys are set, and it is written whole through a temp file and a rename, keeping the keys it does not know. A resident board and a popup that change different settings keep both. The last to change the same key wins. The read and the rename are not locked, so two writes in the same few milliseconds can lose one.
- **Defaults, once:** a missing file is the defaults. A file that cannot be read or is not a JSON object is the defaults too, said once in the footer and never retried; the next change replaces it. An extension's view or sort that is not there yet stays in the file until it is.

## Consequences

- One more file in the state dir, named in [system architecture](../system-architecture.md#files). To reset, delete it.
- A status or `/` text that hides every row can greet the next start. The top bar names the status, and the footer says what clears each: `esc` or `ff` on the board; in `pick` and `board --once`, where `esc` leaves, `ff` for the status and `/` to edit the text.
- `pick` and the board never write the same file.

## Supersedes

None. It amends [17](0017-pick-keeps-its-view-and-sort-in-its-own-file.md): the board keeps filters too.
