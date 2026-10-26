# TUI

`rexboard` with no command in a terminal, or `rexboard board`, opens the board. It is a client of `rexboard serve`: it subscribes to serve's rows, and `s` goes through serve too, so every board and `ls` share one `seen`.

The board is built to live in a narrow column, so the narrow layout is the main one.

## Layout

- **Top bar:** `rexboard`, the number of hosts, panes and agents, the clock and the hosts. A host that is not `up` shows in red with its state (`mini down`), and a stopped serve shows `serve stopped · r reconnects` (while the board first connects, a dim `connecting to serve`). When serve cannot be started, the list says why. When the line is short, the host list goes first, then the counts, then the clock; the red alerts always stay.
- **Groups**, in order, each with its count and a dashed rule: `NEEDS YOU` (blocked, error, unseen done), `WORKING`, `QUIET` (idle, seen done) and `UNREPORTED` (no status record, unknown state, a host that is down, or a suspect `working` report). Empty groups are not drawn. Inside `NEEDS YOU`, blocked comes first, then error, then unseen done, each oldest first. A folded group shows `▸` before its name.
- **Column header:** one dim line over the list names each column, at the same widths as the rows (the glyph column has no name). A blank row separates it from the top bar. `pick` has both too. On a screen under 12 rows the header and the blank row are left out, so a short screen keeps every line for rows.
- **Rows:** a glyph, then the columns the width allows:

  | Width | Columns |
  | --- | --- |
  | under 60 | `AGE` · `WHERE` (`session › block`) · `MSG` |
  | 60-99 | adds `HARNESS` and `HOST` |
  | 100-139 | adds `STATE`, and the window in `WHERE`: `session › window › block`; with a 38-column pane it splits from 132 (see below) |
  | 140 and up | splits `WHERE` into `SESSION`, `WINDOW` and `BLOCK` once the list has 93 columns beside the detail pane (from 140 with the default third), and adds `TURN` once it has 101 (from 152 with the default third, from 140 with a 38-column pane) |

  Beside the detail pane the list picks these columns by its own width, as if the pane were 38 columns, so a wider pane never squeezes `WHERE` (or its three parts together) and `MSG` below their widths at 100 columns. `pick` has no pane and picks them by the board's width: the split from 132, `TURN` from 140.

  Each column answers one question:

  | Column | Question | Shows |
  | --- | --- | --- |
  | `STATE` | What needs me? | The kind when the row has one (`permission`, `question`), since the red glyph already says blocked; else the state word. A suspect row reads `idle?` |
  | `AGE` | How long has it been like this? | Time in state |
  | `TURN` | How long did its last turn take? | The last working turn, `-` before the first one ends |
  | `HARNESS` | Which agent? | `claude`, `codex`, ... |
  | `HOST` | Which machine? | The Rex host |
  | `WHERE` | Which pane? | `session › window › block`, until the split |
  | `SESSION` | Which session? | The Rex session |
  | `WINDOW` | Which window? | The Rex window (a tab in the Rex app) |
  | `BLOCK` | Which pane? | The block's label |
  | `MSG` | What did it say? | The record's message, else the title |

  Glyphs: `◉` blocked, `✖` error, `●` done, `◐` working, `○` idle or seen done, `·` unknown. Blocked and error are red, unseen done green, working yellow, the rest dim. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`). A `working` report more than 120 seconds old on a pane that looks idle (an idle `✳ ` title, or a shell back in front) is not trusted: the row shows `idle?` (from 100 columns; narrower, its `○` glyph), dim, under `UNREPORTED`, and the detail says `may be stale:` and why. The check cannot see inside tmux: with `tmux` in front the title is tmux's, so only a shell back in front makes such a row suspect. serve checks this every second, since time alone can make a report suspect. Rows of a host that is down, and every row while serve is stopped, are dim. The selected row has a raised background. A pane labelled `shell`, or named after its foreground process, tells you nothing, so its path is `session › window` (also below 100 columns), and its `BLOCK` is its title, else its window. When a path is too long, the window is cut first (to `…` below 6 columns), then the session down to an 8-column stem, then the last label. Rows on one host that would still read the same, as shown, end in `#N`, their place among those rows by window and then by block age. A number can shift when a twin's title changes and it stops reading the same; once split the `#N` goes on `BLOCK`, and a long session, window or block keeps both ends with `…` in the middle. Below 60 columns, where there is no host column, a row from a host other than `local` also gets `@HOST` when a twin is on another host; at 50 columns that reads `api…way › pair #1`, `api…way › pair #2` and `ap…y › pair @mini`. The suffix is never cut: the path gives way first, down to `…`. When rows do not fit, the last line says how many are below (`↓ 4 more`).
- **Detail pane**, for the selected row: at 100 columns and up on the right (a third of the width, at least 38 columns; `<` and `>` resize it, see [Detail width](#detail-width)), with the block label (for a generic one, the title, else the window), then `state` (with its kind, as in `blocked · permission`, and `(reported)` when the report is suspect), `in state`, `msg` (wrapped to three lines on the right, two underneath), `host`, `session`, `window`, `title` (without its leading status glyph), `harness`, `source` and `last rpt` (the time of the last report, or `none`). Below 100 columns it sits underneath the list, sized to its content up to half the screen, and leaves out `in state`, and `msg` when the row shows it whole.
- **Footer:** the keys that fit, most needed first (`j/k move · s seen · / filter · ? keys` at 50 columns; from 54, `v view` shows after `/ filter`), or the filter being typed, or a message for 3 seconds. An open peek starts the footer with `space closes ·` (and drops `space peek`), and a kept filter with `/mini · esc clears ·`. A filter that matches nothing says `no rows match "mini" · esc clears` in the list. Below 100 columns a rule sits above the footer when the screen is 20 rows or more.

Long labels are cut to fit: a session, window or block label to 24 columns with `…` in the middle, and every cell at its column's edge with `…`. Widths count CJK and emoji as two columns. Colours are ANSI named colours only, so the terminal's theme carries through.

## Views

A view is a way of grouping the rows plus a sort. The view and sort last for the run.

| View | Groups | Order inside a group, with no sort |
| --- | --- | --- |
| `status` (the default) | `NEEDS YOU`, `WORKING`, `QUIET`, `UNREPORTED`, as above | As above |
| `flat` | None: one list with no group headers | The `status` view's order |
| `harness` | One per harness (`CLAUDE`, `CODEX`), a to z, with `?` (no harness) last | The `status` view's order |
| `host` | One per host (`LOCAL`, `MINI`), a to z | The `status` view's order |

Shift and a column's letter sorts by that column inside each group: `S` state, `A` age, `T` turn, `R` harness (runtime), `O` host, `W` where, `E` session, `N` window, `B` block, `M` msg. `W` sorts by the whole `session › window › block`, and `E`, `N` and `B` by one part, whether or not `WHERE` is split. Sorting works as in k9s: Shift and the letter, again to flip. The first press sorts `AGE` and `TURN` longest first (`↓`), `STATE` most urgent first (blocked, error, done, working, idle; `↑`) and the text columns a to z (`↑`). The same letter again flips it, and `esc` clears it. A row with no value (no status record to date it, no finished turn) sorts last either way. In the `status` view, `NEEDS YOU` keeps its blocked rows first under any sort. `T` sorts by turn even when the `TURN` column is not drawn. In the `host` view rows leave out `@HOST`, since their group names the host.

The column header underlines each column's sort letter and puts `↑` or `↓` just left of the sorted column's name. When the view is not `status`, or a sort is set, the top bar says so after `rexboard`: `view: flat ↓age`. The selection stays on its row when the view or the sort changes.

## Keys

| Key | Does |
| --- | --- |
| `j` / `k`, arrows | Move between rows and folded groups; the ends wrap |
| `g` / `G` | First / last |
| `1`-`9` | Select the nth `NEEDS YOU` row, counted in the order shown (so a sort changes which row a digit picks), opening its group if it is folded |
| `/` | Filter rows by host, session, window, block, title, state, message or harness; `enter` keeps it, `esc` clears it |
| `f`, then a letter | Show one status only (see [Status filter](#status-filter)). `f f` or `esc` clears it |
| `space` | Peek: the pane's newest lines, from `rex capture` (it keeps the last 200), with escape sequences removed. Long lines wrap, so the newest ones show whole, and the oldest drop off the top; the newest line is always on the last row. From 100 columns it fills the detail pane under the fields, headed `peek · HH:MM:SS`, the time of the capture. Below 100 it takes the fields' place, in up to half the screen, and the name line reads `PANE · peek · HH:MM:SS`. It is one capture, taken on `space`: a resize shows more or fewer lines from it. `space` again closes it |
| `s` | Mark the selected pane seen, through serve. The message names it `session › block` (`session › window` for a generic block label). A blocked or erroring pane stays in `NEEDS YOU`, and the message says so |
| `ctrl+s` | Mark every unseen `done` row the list shows seen (a kept filter narrows it), through serve, and say how many (`9 done rows marked seen`). Blocked and erroring rows are left alone. Before v0.2 this was `S`, which now sorts by state; the `?` table says `(was S)`, and the first `S` of a run says `sorted by state · bulk seen is ctrl+s now` |
| `v` | Next view: `status`, `flat`, `harness`, `host`, then `status` again |
| `:` | Open a view by name. Tab completes a name (`fl` to `flat`), Enter opens it (or the one view the text starts), Esc cancels. An unknown name says `no view "nope"` |
| `S` `A` `T` `R` `O` `W` `E` `N` `B` `M` | Sort by `STATE`, `AGE`, `TURN`, `HARNESS` (runtime), `HOST`, `WHERE`, `SESSION`, `WINDOW`, `BLOCK` or `MSG`. The same letter again flips the direction. See [Views](#views) |
| `z` | Fold the selected row's group, or open the folded group the selection is on. `NEEDS YOU` opens by itself when a new row enters it |
| `<` / `>` | Narrow / widen the detail pane by 5% of the width, from 100 columns. The width is kept per machine (see [Detail width](#detail-width)) |
| `?` | Show every key in the detail pane; `?` or `esc` hides it |
| `enter` | Go to the pane in the Rex window you are in, on its host, and mark it seen, as [`rexboard jump`](cli.md#rexboard-jump-next-and-pick) does. The footer says `going to SESSION › PANE…` until the jump ends, then where it went; Enter during a jump says one is already running. A failed jump's reason stays in the footer until the next key, and in full in the detail pane. The board stays open |
| `r` | Reconnect to serve |
| `esc` | Clear the `/` filter, the status filter and the sort, close the peek and the key table, and open every folded group |
| `q`, `ctrl-c` | Quit |

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state. SIGTERM and SIGHUP (closing its window) quit it the same way as `q`, so the terminal is left as it was, and the board exits on its own within a second if its terminal goes away.

## Detail width

From 100 columns the detail pane sits on the right. It starts at a third of the width, but never under 38 columns: 38 at 100 columns, 46 at 140, 66 at 200. `<` narrows it and `>` widens it by 5% of the width per press. It never leaves the list fewer than 61 columns, the list's width at 100 columns, so at 100 columns the pane is always 38, and at 140 it runs from 38 to 78. A press at either end changes nothing and says `the detail pane is as wide as it goes` or `the detail pane is as narrow as it goes`.

The width is kept as a share of the board's width, so it follows the window when that resizes. It lives in `tui.json` beside serve's `state.json` (`~/.local/state/rexboard/tui.json`, or under `XDG_STATE_HOME`), is read when the board starts and written on each press. A missing or unreadable `tui.json` means the default third. Below 100 columns, where the pane sits under the list, `<` and `>` only say `< and > size the detail pane on boards wider than 100 columns`; `pick` has no pane and ignores them.

## Status filter

`f` then one letter shows only the rows with that status:

| Letter | Shows | Rows |
| --- | --- | --- |
| `w` | working | the `WORKING` group: `state` `working`, trusted |
| `d` | done | `state` `done`, seen or not |
| `q` | question | `state` `blocked` with `kind` `question` |
| `p` | permission | `state` `blocked` with any other `kind` (agents send `permission`) or none |
| `e` | error | `state` `error` |
| `u` | unreported | the `UNREPORTED` group: no status record, `unknown`, a stale host or a suspect report |
| `i` | idle | `state` `idle` |

A row in `UNREPORTED` matches only `u`, so each row matches exactly one letter. While `f` waits for its letter, the footer lists each status with how many rows it would show, its letter underlined as in the column header: `f working 3 · done 5 · … · f/esc clears`. Narrower, the hint goes and the separators tighten, then it reads `f w3 d5 … · f/esc clears`. `f f` or `esc` clears the filter. Another letter leaves the filter as it was and says which letters `f` takes; a key that is not a character (an arrow, `enter`) does the same without a word. A key with `ctrl` or `alt` keeps its own meaning, so `ctrl+c` still quits. When nothing has the status, the list says `no error rows · f f or esc clears` (`no error rows match "x"` with a `/` text too). The top bar shows the filter after `rexboard` (`only question`, or `view: flat ↓age · only question`).

The filter combines with the `/` text and with every view. Setting it opens folded groups, so the rows asked for are not hidden. `1`-`9` and `ctrl+s` count only the rows the filters show. It works in `pick` too, where a plain `esc` exits as usual and `f esc` clears the filter.

## Pick

`rexboard pick` shows the same list, with the same column header, without the detail pane, at every width. Enter goes to the selected pane and exits 0; esc exits 1. `/` filters, as on the board.
