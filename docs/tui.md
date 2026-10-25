# TUI

`rexboard` with no command in a terminal, or `rexboard board`, opens the board. It is a client of `rexboard serve`: it subscribes to serve's rows, and `s` goes through serve too, so every board and `ls` share one `seen`.

The board is built to live in a narrow column, so the narrow layout is the main one.

## Layout

- **Header:** `rexboard`, the number of hosts, panes and agents, the clock and the hosts. A host that is not `up` shows in red with its state (`mini down`), and a stopped serve shows `serve stopped · r reconnects` (while the board first connects, a dim `connecting to serve`). When serve cannot be started, the list says why. When the line is short, the host list goes first, then the counts, then the clock; the red alerts always stay.
- **Groups**, in order, each with its count and a dashed rule: `NEEDS YOU` (blocked, error, unseen done), `WORKING`, `QUIET` (idle, seen done) and `UNREPORTED` (no status record, unknown state, a host that is down, or a suspect `working` report). Empty groups are not drawn. Inside `NEEDS YOU`, blocked comes first, then error, then unseen done, each oldest first. A folded group shows `▸` before its name.
- **Column header:** one dim line over the list names each column, at the same widths as the rows (the glyph column has no name). `pick` has it too. On a screen under 12 rows it is left out, so a short screen keeps every line for rows.
- **Rows:** a glyph, then the columns the width allows:

  | Width | Columns |
  | --- | --- |
  | under 60 | `AGE` · `WHERE` (`session › pane`) · `MSG` |
  | 60-99 | adds `HARNESS` and `HOST` |
  | 100-139 | adds `STATE`, and the tab in `WHERE`: `session › tab › pane` |
  | 140 and up | adds `TURN` |

  Each column answers one question:

  | Column | Question | Shows |
  | --- | --- | --- |
  | `STATE` | What needs me? | The kind when the row has one (`permission`, `question`), since the red glyph already says blocked; else the state word. A suspect row reads `idle?` |
  | `AGE` | How long has it been like this? | Time in state |
  | `TURN` | How long did its last turn take? | The last working turn, `-` before the first one ends |
  | `HARNESS` | Which agent? | `claude`, `codex`, ... |
  | `HOST` | Which machine? | The Rex host |
  | `WHERE` | Which pane? | `session › tab › pane` |
  | `MSG` | What did it say? | The record's message, else the title |

  Glyphs: `◉` blocked, `✖` error, `●` done, `◐` working, `○` idle or seen done, `·` unknown. Blocked and error are red, unseen done green, working yellow, the rest dim. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`). A `working` report more than 120 seconds old on a pane that looks idle (an idle `✳ ` title, or a shell back in front) is not trusted: the row shows `idle?` (from 100 columns; narrower, its `○` glyph), dim, under `UNREPORTED`, and the detail says `may be stale:` and why. The check cannot see inside tmux: with `tmux` in front the title is tmux's, so only a shell back in front makes such a row suspect. serve checks this every second, since time alone can make a report suspect. Rows of a host that is down, and every row while serve is stopped, are dim. The selected row has a raised background. A pane labelled `shell`, or named after its foreground process, tells you nothing, so its path is `session › tab` (also below 100 columns). When a path is too long, the tab is cut first (to `…` below 6 columns), then the session down to an 8-column stem, then the last label. Rows that would still read the same end in `#N`, the pane's place among its tab's agent panes. Below 60 columns, where there is no host column, a row from a host other than `local` also gets `@HOST` when a twin is on another host; at 50 columns that reads `api…way › pair #1`, `api…way › pair #2` and `ap…y › pair @mini`. The suffix is never cut: the path gives way first, down to `…`. When rows do not fit, the last line says how many are below (`↓ 4 more`).
- **Detail pane**, for the selected row: at 100 columns and up on the right (38 columns), with the pane label (for a generic one, the title, else the tab), then `state` (with its kind, as in `blocked · permission`, and `(reported)` when the report is suspect), `in state`, `msg` (wrapped to three lines on the right, two underneath), `host`, `session`, `tab`, `title` (without its leading status glyph), `harness`, `source` and `last rpt` (the time of the last report, or `none`). Below 100 columns it sits underneath the list, sized to its content up to half the screen, and leaves out `in state`, and `msg` when the row shows it whole.
- **Footer:** the keys that fit, most needed first (`j/k move · s seen · / filter · ? keys` at 50 columns), or the filter being typed, or a message for 3 seconds. An open peek starts the footer with `space closes ·` (and drops `space peek`), and a kept filter with `/mini · esc clears ·`. A filter that matches nothing says `no rows match "mini" · esc clears` in the list. Below 100 columns a rule sits above the footer when the screen is 20 rows or more.

Long labels are cut to fit: a `session`, `tab` or `pane` label to 24 columns with `…` in the middle, and every cell at its column's edge with `…`. Widths count CJK and emoji as two columns. Colours are ANSI named colours only, so the terminal's theme carries through.

## Keys

| Key | Does |
| --- | --- |
| `j` / `k`, arrows | Move between rows and folded groups; the ends wrap |
| `g` / `G` | First / last |
| `1`-`9` | Select the nth `NEEDS YOU` row, opening the group if it is folded |
| `/` | Filter rows by host, session, tab, pane, title, state, message or harness; `enter` keeps it, `esc` clears it |
| `space` | Peek: the last 12 lines of the pane, from `rex capture`, with escape sequences removed. It shows in the detail pane, in place of its fields below 100 columns. `space` again closes it |
| `s` | Mark the selected pane seen, through serve. The message names it `session › pane` (`session › tab` for a generic pane label). A blocked or erroring pane stays in `NEEDS YOU`, and the message says so |
| `S` | Mark every unseen `done` row the list shows seen (a kept filter narrows it), through serve, and say how many (`9 done rows marked seen`). Blocked and erroring rows are left alone |
| `z` | Fold the selected row's group, or open the folded group the selection is on. `NEEDS YOU` opens by itself when a new row enters it |
| `?` | Show every key in the detail pane; `?` or `esc` hides it |
| `enter` | Go to the pane in the main Rex window, on its host, and mark it seen, as [`rexboard jump`](cli.md#rexboard-jump-next-and-pick) does. The footer says `going to SESSION › PANE…` until the jump ends, then where it went; Enter during a jump says one is already running. A failed jump's reason stays in the footer until the next key, and in full in the detail pane. The board stays open |
| `r` | Reconnect to serve |
| `esc` | Clear the filter, close the peek and the key table, and open every folded group |
| `q`, `ctrl-c` | Quit |

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state. SIGTERM and SIGHUP (closing its window) quit it the same way as `q`, so the terminal is left as it was, and the board exits on its own within a second if its terminal goes away.

## Pick

`rexboard pick` shows the same list, with the same column header, without the detail pane, at every width. Enter goes to the selected pane and exits 0; esc exits 1. `/` filters, as on the board.
