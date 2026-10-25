# TUI

`rexboard` with no command in a terminal, or `rexboard board`, opens the board. It is a client of `rexboard serve`: it subscribes to serve's rows, and `s` goes through serve too, so every board, `ls` and `next` share one `seen`.

The board is built to live in a narrow column, so the narrow layout is the main one.

## Layout

- **Header:** `rexboard`, then the number of hosts, panes and agents. On the right, the clock and each host, in red with its state when it is not `up`, or `serve stopped, r reconnects`.
- **Groups**, in order, each with its count and a dashed rule: `NEEDS YOU` (blocked, error, unseen done), `WORKING`, `QUIET` (idle, seen done) and `UNREPORTED` (no status record, unknown state, or a host that is down). Empty groups are not drawn. Inside `NEEDS YOU`, blocked comes first, then error, then unseen done, each oldest first.
- **Rows:** a glyph, then the columns the width allows:

  | Width | Columns |
  | --- | --- |
  | under 60 | glyph · time in state · pane · message |
  | 60-99 | adds harness and host |
  | 100 and up | adds the state word and `session › tab › pane` |

  Glyphs: `◉` blocked, `✖` error, `●` done, `◐` working, `○` idle or seen done, `·` unknown. Blocked and error are red, unseen done green, working yellow, the rest dim. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`). Rows of a host that is down are dim. The selected row has a raised background.
- **Detail pane**, for the selected row: at 100 columns and up on the right (38 columns), below that underneath the list. It shows the pane label, then `host`, `session`, `tab`, `title`, `harness`, `source`, `state` (with its kind, as in `blocked · permission`), `in state`, `last rpt` (the time of the last report, or `none`) and `msg`.
- **Footer:** the keys, or the filter being typed, or a message for 3 seconds.

Long labels are cut to fit: a `session`, `tab` or `pane` label to 24 columns with `…` in the middle, and every cell at its column's edge with `…`. Widths count CJK and emoji as two columns. Colours are ANSI named colours only, so the terminal's theme carries through.

## Keys

| Key | Does |
| --- | --- |
| `j` / `k`, arrows | Move between rows; headers are skipped and the ends wrap |
| `g` / `G` | First / last row |
| `1`-`9` | Select the nth `NEEDS YOU` row |
| `/` | Filter rows by host, session, tab, pane, title, state, message or harness; `enter` keeps it, `esc` clears it |
| `space` | Peek: the last 12 lines of the pane (`rex capture`), shown in the detail pane. `space` again closes it |
| `s` | Mark the selected pane seen, through serve |
| `z` | Fold or unfold the selected row's group |
| `enter` | Jump to the pane. Arrives in a later version; for now it says so |
| `r` | Reconnect to serve |
| `esc` | Clear the filter and the peek |
| `q`, `ctrl-c` | Quit |

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state.
