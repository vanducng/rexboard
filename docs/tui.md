# TUI

`rexboard` with no command in a terminal, or `rexboard board`, opens the board. It is a client of `rexboard serve`: it subscribes to serve's rows, and `s` goes through serve too, so every board and `ls` share one `seen`.

The board is built to live in a narrow column, so the narrow layout is the main one.

## Layout

- **Header:** `rexboard`, the number of hosts, panes and agents, the clock and the hosts. A host that is not `up` shows in red with its state (`mini down`), and a stopped serve shows `serve stopped · r reconnects` (while the board first connects, a dim `connecting to serve`). When serve cannot be started, the list says why. When the line is short, the host list goes first, then the counts, then the clock; the red alerts always stay.
- **Groups**, in order, each with its count and a dashed rule: `NEEDS YOU` (blocked, error, unseen done), `WORKING`, `QUIET` (idle, seen done) and `UNREPORTED` (no status record, unknown state, a host that is down, or a suspect `working` report). Empty groups are not drawn. Inside `NEEDS YOU`, blocked comes first, then error, then unseen done, each oldest first. A folded group shows `▸` before its name.
- **Rows:** a glyph, then the columns the width allows:

  | Width | Columns |
  | --- | --- |
  | under 60 | glyph · time in state · session › pane · message |
  | 60-99 | adds harness and host |
  | 100 and up | adds the state word and the tab: `session › tab › pane` |

  Glyphs: `◉` blocked, `✖` error, `●` done, `◐` working, `○` idle or seen done, `·` unknown. Blocked and error are red, unseen done green, working yellow, the rest dim. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`). A `working` report more than 120 seconds old on a pane that looks idle (an idle `✳ ` title, or a shell back in front) is not trusted: the row shows `idle?` (from 100 columns; narrower, its `○` glyph), dim, under `UNREPORTED`, and the detail says `may be stale:` and why. The check cannot see inside tmux: with `tmux` in front the title is tmux's, so only a shell back in front makes such a row suspect. serve checks this every second, since time alone can make a report suspect. Rows of a host that is down, and every row while serve is stopped, are dim. The selected row has a raised background. A pane labelled `shell`, or with the name of its foreground process, says nothing, so its path is `session › tab` (also below 100 columns), unless another agent pane shares its tab. When a path is too long, the tab is cut first, then the session; the last label stays whole, and a label cut below 6 columns shows as `…`. When rows do not fit, the last line says how many are below (`↓ 4 more`).
- **Detail pane**, for the selected row: at 100 columns and up on the right (38 columns), with the pane label (for a generic one, the title, else the tab), then `state` (with its kind, as in `blocked · permission`, and `(reported)` when the report is suspect), `in state`, `msg`, `host`, `session`, `tab`, `title` (without its spinner glyph), `harness`, `source` and `last rpt` (the time of the last report, or `none`). Below 100 columns it sits underneath the list, sized to its content up to half the screen, and leaves out `in state`, and `msg` when the row shows it whole; a message the row cuts is wrapped to two lines.
- **Footer:** the keys that fit, most needed first (`j/k move · s seen · / filter · ? keys` at 50 columns), or the filter being typed, or a message for 3 seconds. An open peek starts the footer with `space closes ·`, and a kept filter with `/mini · esc clears ·`. A filter that matches nothing says `no rows match "mini" · esc clears` in the list. Below 100 columns a rule sits above the footer when the screen is 20 rows or more.

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
| `S` | Mark every unseen `done` row seen, through serve, and say how many (`9 done rows marked seen`). Blocked and erroring rows are left alone |
| `z` | Fold the selected row's group, or open the folded group the selection is on. `NEEDS YOU` opens by itself when a new row enters it |
| `?` | Show every key in the detail pane; `?` or `esc` hides it |
| `enter` | Jump to the pane. Arrives in a later version; for now it says so |
| `r` | Reconnect to serve |
| `esc` | Clear the filter, close the peek and the key table, and open every folded group |
| `q`, `ctrl-c` | Quit |

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state. SIGTERM and SIGHUP (closing its window) quit it the same way as `q`, so the terminal is left as it was, and the board exits on its own within a second if its terminal goes away.
