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
  | 100-131 | adds `STATE`, and the window in `WHERE`: `session › window › block` |
  | 132-139 | splits `WHERE` into `SESSION`, `WINDOW` and `BLOCK` |
  | 140 and up | adds `TURN` |

  The list is as wide as the board at every width (the turn card sits under it), and `pick` uses the same columns.

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
  | `MSG` | What did it say? | The agent's own message, its first line. Blank when there is none, and for a hook's filler (`Done`, `Working`, `Allow <tool>?`); the turn card has the title. See [Agent status](agent-status.md) |

  Glyphs: `◉` blocked, `✖` error, `●` done, `◐` working, `○` idle or seen done, `·` unknown. Blocked and error are red, unseen done green, working yellow, the rest dim. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`). A `working` report more than 120 seconds old on a pane that looks idle (an idle `✳ ` title, or a shell back in front) is not trusted: the row shows `idle?` (from 100 columns; narrower, its `○` glyph), dim, under `UNREPORTED`, and the turn card says `may be stale:` and why. The check cannot see inside tmux: with `tmux` in front the title is tmux's, so only a shell back in front makes such a row suspect. serve checks this every second, since time alone can make a report suspect. Rows of a host that is down, and every row while serve is stopped, are dim. The selected row has a raised background. A pane labelled `shell`, or named after its foreground process, tells you nothing, so its path is `session › window` (also below 100 columns), and its `BLOCK` is its title, else its window. When a path is too long, the window is cut first (to `…` below 6 columns), then the session down to an 8-column stem, then the last label. Rows on one host that would still read the same, as shown, end in `#N`, their place among those rows by window and then by block age. A number can shift when a twin's title changes and it stops reading the same; once split the `#N` goes on `BLOCK`, and a long session, window or block keeps both ends with `…` in the middle. Below 60 columns, where there is no host column, a row from a host other than `local` also gets `@HOST` when a twin is on another host; at 50 columns that reads `api…way › pair #1`, `api…way › pair #2` and `ap…y › pair @mini`. The suffix is never cut: the path gives way first, down to `…`. When rows do not fit, the last line says how many are below (`↓ 4 more`).
- **Turn card**, for the selected row, under the list at full width, at every width (see [Turn card](#turn-card)). `c` hides it.
- **Footer:** the keys that fit, most needed first (`j/k move · s seen · / filter · ? keys` at 50 columns, then `tab next`, `enter jump`, `a answer`, `space zoom` and `v view` as they fit, so 80 columns ends at `enter jump`; `v view` shows after `/ filter`), or the filter being typed, or a message for 3 seconds. A zoomed card starts the footer with `space returns ·` (and drops `space zoom`), and a kept filter with `/mini · esc clears ·`. While `c` has the card hidden, `c card` follows `? keys`. It goes before `space zoom` when the footer is narrow, so at 80 columns the footer shows `c card` and drops `space zoom`. A filter that matches nothing says `no rows match "mini" · esc clears` in the list. With the card hidden, below 100 columns, a rule sits above the footer when the screen is 20 rows or more.

Long labels are cut to fit: a session, window or block label to 24 columns with `…` in the middle, and every cell at its column's edge with `…`. Widths count CJK and emoji as two columns. Colours are ANSI named colours only, so the terminal's theme carries through.

## Views

A view is a way of grouping the rows plus a sort. The view and sort last for the run.

| View | Groups | Order inside a group, with no sort |
| --- | --- | --- |
| `status` (the default) | `NEEDS YOU`, `WORKING`, `QUIET`, `UNREPORTED`, as above | As above |
| `table` | None: one list with no group headers, sortable by any column with a key or a click | The `status` view's order |
| `harness` | One per harness (`CLAUDE`, `CODEX`), a to z, with `?` (no harness) last | The `status` view's order |
| `host` | One per host (`LOCAL`, `MINI`), a to z | The `status` view's order |

Shift and a column's letter sorts by that column inside each group: `S` state, `A` age, `T` turn, `R` harness (runtime), `O` host, `W` where, `E` session, `N` window, `B` block, `M` msg. `W` sorts by the whole `session › window › block`, and `E`, `N` and `B` by one part, whether or not `WHERE` is split. Sorting works like a table header: Shift and the letter sorts, again flips, a third time clears. The first press sorts `AGE` and `TURN` longest first (`↓`), `STATE` most urgent first (blocked, error, done, working, idle; `↑`) and the text columns a to z (`↑`). The same letter again flips it; a third press, or `esc`, clears it. A row with no value (no status record to date it, no finished turn) sorts last either way. In the `status` view, `NEEDS YOU` keeps its blocked rows first under any sort. `T` sorts by turn even when the `TURN` column is not drawn. In the `host` view rows leave out `@HOST`, since their group names the host.

The column header underlines each column's sort letter and puts `↑` or `↓` just left of the sorted column's name, or just after it when only one space separates it from the column before (`HARNESS HOST↑` at 100 columns). When the view is not `status`, or a sort is set, the top bar says so after `rexboard`: `view: table ↓age`. The selection stays on its row when the view or the sort changes.

In the `table` view a click on a column's header sorts by that column, as its letter does: once to sort, again to flip, a third time to clear. A click on the arrow acts as its column's letter: it flips the sort, or clears it if it is already flipped. A click on a row selects it, and the list keeps its place, so the row stays under the pointer. The wheel moves the selection as `k` and `j` do, but stops at the ends. While the `/`, `:` or `f` prompt is open, clicks and the wheel do nothing. Other clicks do nothing. The first time the view opens in a run, the footer says `click a header to sort · hold Option (iTerm2) or Shift to select text`. Reading clicks needs mouse capture, which stops the terminal's own text selection, so rexboard turns it on only while the `table` view shows and off as you leave it or quit, including on a signal or a panic. In other views, and in `pick`, text selection works as usual and sorting is by letter only. Many terminals still select text in the `table` view with a modifier held (Option in iTerm2, Shift in many others). `flat`, the view's old name, still opens it from `:`.

## Keys

| Key | Does |
| --- | --- |
| `j` / `k`, arrows | Move between rows and folded groups; the ends wrap |
| `g` / `G` | First / last |
| `1`-`9` | Select the nth `NEEDS YOU` row, counted in the order shown (so a sort changes which row a digit picks), opening its group if it is folded |
| `/` | Filter rows by host, session, window, block, title, state, message or harness; `enter` keeps it, `esc` clears it |
| `f`, then a letter | Show one status only (see [Status filter](#status-filter)). `f f` or `esc` clears it |
| `space` | Zoom the card to the whole body and read the end of the pane: its last 48 KiB, the last 200 lines kept, with escape sequences removed, newest at the bottom. A host that does not answer in 3 seconds shows `the pane did not answer within 3s`, and a newer read replaces one still running. `space` again or `esc` returns |
| `tab` | The next row that needs you, after the selected one, round to the first. Leaving a `done` row whose card showed for at least a second marks it seen; a blocked or erroring row is never marked |
| `s` | Mark the selected pane seen, through serve. The message names it `session › block` (`session › window` for a generic block label). A blocked or erroring pane stays in `NEEDS YOU`, and the message says so |
| `ctrl+s` | Mark every unseen `done` row the list shows seen (a kept filter narrows it), through serve, and say how many (`9 done rows marked seen`). Blocked and erroring rows are left alone. Before v0.2 this was `S`, which now sorts by state; the `?` table says `(was S)`, and the first `S` of a run says `sorted by state · bulk seen is ctrl+s now` |
| `v` | Next view: `status`, `table`, `harness`, `host`, then `status` again |
| `:` | Open a view by name. Tab completes a name (`ta` to `table`), Enter opens it (or the one view the text starts), Esc cancels. An unknown name says `no view "nope"` |
| `S` `A` `T` `R` `O` `W` `E` `N` `B` `M` | Sort by `STATE`, `AGE`, `TURN`, `HARNESS` (runtime), `HOST`, `WHERE`, `SESSION`, `WINDOW`, `BLOCK` or `MSG`. The same letter again flips the direction, and a third time clears the sort. See [Views](#views) |
| click, wheel | In the `table` view: a click on a column header sorts by it, as its letter does; a click on a row selects it; the wheel moves the selection |
| `z` | Fold the selected row's group, or open the folded group the selection is on. `NEEDS YOU` opens by itself when a new row enters it |
| `c` | Hide or show the turn card. Hidden, the list fills the body, the footer shows `c card`, and the choice is kept in `tui.json`. A zoom, the key table or a failed jump's reason still opens the card until it goes. `pick` has no card and ignores `c` |
| `?` | Show every key in the card, in as many columns as fit; `?` or `esc` hides it |
| `enter` | Go to the pane in the Rex window you are in, on its host, and mark it seen, as [`rexboard jump`](cli.md#rexboard-jump-next-and-pick) does. The footer says `going to SESSION › PANE…` until the jump ends, then where it went; Enter during a jump says one is already running. A failed jump's reason stays in the footer until the next key, and in full in the turn card. The board stays open |
| `a` | Answer the selected row from the board: pick a dialog's option or reply with a line. See [Answer mode](#answer-mode) |
| `r` | Reconnect to serve |
| `esc` | Clear the `/` filter, the status filter and the sort, end a zoom, close the key table, and open every folded group |
| `q`, `ctrl-c` | Quit (in answer mode, `ctrl-c` only leaves it) |

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state. SIGTERM and SIGHUP (closing its window) quit it the same way as `q`, so the terminal is left as it was, and the board exits on its own within a second if its terminal goes away.

## Turn card

The card is the selected row in full, under the list, as wide as the board:

```
━━ ● done · seat-a · api › main · mini · claude · ended 4m ago · turn 6m12s ━━━━━━ captured 11:21:14 · 8s ━━
 Opened PR #31; CI is green.
   Pushed feat/turn-card and opened PR #31.
   Next step is yours: review and merge.
 title seat-a · source native
```

1. **The bar:** the glyph, the state (with its kind, as in `blocked · permission`), the block, `session › window`, the host and the harness, then how the turn stands: `ended 4m ago · turn 6m12s` for a done row, `waiting 12s` for a blocked one, `failed 2m ago`, or `for 2m` in any other state. When the bar is short of room, the window, then the harness, the host and the place give way, never the outcome. On the right, how fresh the pane's tail is: `captured 11:21:14 · 8s`, or `newer report · space refreshes` once the row has reported since.
2. **The headline**, in bold: the message's first line (see [Agent status](agent-status.md)), else the title.
3. **The ask or the body:** the message's other lines, wrapped. A blocked row's ask is never cut: its card grows as far as the list's last 3 rows.
4. A failed jump's reason, `may be stale:` for a suspect row, and the title and `source`.
5. The pane's tail, when it has been read, newest line at the bottom.

A row with no status record says `no status from this pane` and `space captures the screen`.

The card is 8 rows, or 40% of the body when that is more, and always leaves the list 3 rows under its column header. Text wraps at 120 columns however wide the board. `space` zooms it to the whole body, and `c` hides it.

Reading a card does not mark a row seen. `s` does, and so does `tab` off a `done` row whose card has shown for at least a second. The tail of a blocked row is read by itself once its card has shown for 300 ms, once per report, so the ask can be checked against the screen; moving through rows reads none.

`c`'s choice lives in `tui.json` beside serve's `state.json` (`~/.local/state/rexboard/tui.json`, or under `XDG_STATE_HOME`), as `detail_hidden` (missing means shown). The file is read when the board starts and written on each `c`. The side pane's old width key is ignored, and `<` and `>` say once that they are gone.

## Answer mode

`a` answers the selected row without leaving the board. The board first reads the pane, then the card turns into a red bar naming the target, the pane's last lines and a reply line:

```
━━ ANSWER → seat-a · api-gateway › main · local · claude ━━━━━━━━━━━━━━━━━━━━━━━━━━ captured 11:22:01 · 1s ━━
 │  Do you want to proceed?
 │  ❯ 1. Yes
 │    2. Yes, and don't ask again for git push commands in /work/api-gateway   press 2 twice
 │    3. No, and tell Claude what to do differently
 ›
1-3 pick · esc back to board                       the pane is re-checked before sending · board keys are off
```

| Key | Does |
| --- | --- |
| `1`-`9` | When the screen shows a numbered dialog (the reply line is then a dim `›` with no cursor), picks that option: one key press, no Enter. An option that grants something for later (`always`, `don't ask again`, `auto mode`) needs the same digit again within 3 seconds; the first press only says `press 2 again to send it`. A digit picks only an option the card shows: on a short board the card takes rows from the list, all of them if it must, and an option still off screen is refused (`option 1 is off screen; make the board taller or jump to it`) and left out of the footer's `1-3 pick` |
| text, then `enter` | When the screen shows an input line and no dialog, sends the line as a paste, then Enter. `enter` with nothing typed does nothing. Text into a dialog is refused: `pick a number; this dialog takes no text` |
| `esc` | Clears the typed text (the footer then says `esc clears`), or leaves when there is none |
| `ctrl-c` | Leaves; it never quits the board here |
| `tab` | Leaves and selects the next row that needs you; press `a` again there |

Board keys are off while answering, and nothing but these keys reaches the pane: never Esc, never Ctrl+C.

The mode is bound to the row's host and pane, not its place in the list, so a re-sort cannot move it. It is refused on the board's own pane and on a pane with no status record (`enter` jumps there instead).

Before anything is sent, one `rex do` on the pane's host checks, in order, that:

1. the pane's status report is the one the board read (else `the agent's status changed since it was read; nothing sent`);
2. an agent, not a shell, is in front (else `zsh is in front, not the agent; nothing sent`); a tmux seat must still show the same seat's title;
3. the dialog's options, or the input line and the line above it, are still on the last 40 lines, whole (else `the screen changed since it was read; nothing sent`);
4. for a line, no dialog is up: options 1 and 2 with no input line below them (else `a dialog is up; pick a number; nothing sent`). Text into a dialog would be read as its highlighted option, so this holds whatever the board believes it shows.

A pane that is gone says `the pane is gone; nothing sent`. Only then does the key or the line go out, in the same script. A refusal sends nothing, clears any typed text, keeps the mode open with the reason in red, and reads the pane again. A read that shows a dialog clears typed text too.

Two failures are not refusals. `the pane did not answer within 5s, so it may or may not have the key; look before sending again` means the script ran out of time and the key may have gone out. `the text was pasted but Enter did not go through; it is in the pane's input line, so jump to it` means the paste landed and its Enter did not. Any other failure before the send ends `; nothing sent`. A line is also refused when the input line already holds typed text, which the reply would join (`the input line already has text, which a reply would join; nothing sent`). A dim placeholder or suggested prompt does not count as typed.

After a send the board returns to the same row, marks it seen, and the card's bar says what went where: `sent 1 (Yes) → seat-a 11:22:04`, until the row needs you again with a newer report. Nothing moves on by itself; `tab` goes to the next row that needs you.

Options are read for Claude Code's dialogs (permission prompts and questions). Other harnesses get line replies at an input line they show; a dialog the board does not recognise offers nothing to send, so jump to it.

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

`rexboard pick` shows the same list, with the same column header, without the turn card, at every width. Enter goes to the selected pane and exits 0; esc exits 1. `/` filters, as on the board.
