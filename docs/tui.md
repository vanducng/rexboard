# TUI

`rexboard` with no command in a terminal, or `rexboard board`, opens the board. It is a client of `rexboard serve`: it subscribes to serve's rows, and marking a row seen goes through serve too, so every board and `ls` share one `seen`. The board only reads Rex, except to jump to a pane and to answer or close one ([what it writes](system-architecture.md)).

The board is built to live in a narrow column, so the narrow layout is the main one.

## Layout

- **Top bar:** `rexboard`, the number of hosts, panes and agents, the clock and the hosts. A host that is not `up` shows in red with its state (`mini down`), and so does an extension whose last run failed (`ext fake failed`, or `ext fake, other failed`; when they would not fit, `2 ext failed`). A stopped serve shows `serve stopped · R reconnects` (while the board first connects, a dim `connecting to serve`). When serve cannot be started, the list says why. When the line is short, the host list goes first, then the counts, then the clock; the red alerts always stay.
- **Groups**, in order, each with its count and a dashed rule: `NEEDS YOU` (blocked, error, unseen done), `WORKING`, `QUIET` (idle, seen done) and `UNREPORTED` (no status record, unknown state, a host that is down, or a suspect `working` report). Empty groups are not drawn. Inside `NEEDS YOU`, blocked comes first, then error, then unseen done, each oldest first. A folded group shows `▸` before its name.
- **Column header:** one dim line over the list names each column, at the same widths as the rows (the glyph column has no name, and `#` heads the numbers). A blank row separates it from the top bar. `pick` has both too. On a screen under 12 rows the header and the blank row are left out, so a short screen keeps every line for rows.
- **Rows:** a row number, a glyph, then the columns the width allows. The dim `#` column is 3 cells wide at every width and numbers the rows on screen from 1 at the top, up to 99; group headers and the blank rows between groups have no number, and a row past 99 has none. The number is the row's place on screen, so it changes when the list scrolls, re-sorts or is filtered (see `1`-`9` in [Keys](#keys)).

  | Width | Columns |
  | --- | --- |
  | under 60 | `AGE` · `WHERE` (`session › block`) · `MSG` |
  | 60-99 | adds `HARNESS` and `HOST` |
  | 100-131 | adds `STATE`, and the window in `WHERE`: `session › window › block` |
  | 132-139 | splits `WHERE` into `SESSION`, `WINDOW` and `BLOCK`, each as wide as its longest label (cut to 24 columns) and a two-column gap (one when that does not fit; at least 10 columns) while together they stay within WHERE's share; `MSG` takes what they free |
  | 140 and up | adds `TURN`, and `MODEL` and `CTX` while some row reports what its turn used; `TOK`, `COST` and `5H` too when `tui.json` asks for them. Below 160, while they show, `WHERE` stays one column so `MSG` keeps its room |

  The list is as wide as the board at every width (the turn card sits under it), and `pick` uses the same columns.

  `HOST` is as wide as the longest host name shown, from 8 to 16 columns, with `…` past that.

  From 140 columns `MSG` keeps room to read: 40 columns, or 35% of the list when that is more, but never more than the longest message shown needs, so short messages cost no label its width. The floor holds with either `WHERE` or `SESSION`, `WINDOW` and `BLOCK`, and after Nerd marks and the usage columns.

  The other columns give way to it in this order: the location columns (each label cut to 24 columns, then sharing what is left, at least 10 each, and `WHERE` not under 20), then the extension columns, the last one first, then the usage columns, the last one first. The turn card still lists every field of the selected row, so nothing the list drops is lost.

  So a board with extension fields shows fewer of them until it is wide enough. With three, all three show from about 200 columns, earlier when the messages are short.

  **Column widths.** `"widths"` in `tui.json` fixes a column's width in columns, gap included, at every board width where the column shows: `{"widths": {"host": 12, "session": 20, "window": 24, "block": 20, "msg": 60}}`. The names are `host`, `session`, `window`, `block` and `msg`. Another name, or a value that is not a whole number from 1 to 500, is ignored when the list is drawn, and the file keeps what it says, so `c` writes it back unchanged. A fixed `session`, `window` or `block` is not fitted to its labels; the others give way to it before `MSG` does. A fixed `msg` ends `MSG`'s floor: it is that wide, or as wide as the others leave, and no extension column is dropped to make room for it (at 140 columns the extension columns the floor would drop stay). Fixed widths that add up to more than the board are clamped on a best-effort basis: each column is narrowed to what the board has left, in no promised order, and `MSG` keeps at least 5 columns.

  The usage columns are for panes whose harness runs `rexboard usage`, on serve's own host from the pane's file and on any other host from the `usage` record that harness's hook told Rex ([Usage on every host](agent-status.md#usage-on-every-host)); a host's panes show them once rexboard with this is installed there and the agent has started since. A pane keeps its last turn's figures while it lives, however long it idles, since a harness writes only when it works, and a new session in the same pane replaces them with its own. A figure the harness does not report is `-`, never `0`. `TOK`, `COST` and `5H` are off until `tui.json` names them: `"columns": ["tok", "cost", "5h"]` (see [Turn card](#turn-card) for where the file lives). They keep the table's order whatever the list's, and any other name is ignored. From 160 columns `MODEL` widens to the longest model name, up to 20. The figures are right-aligned, so their digits line up.

  Each column answers one question:

  | Column | Question | Shows |
  | --- | --- | --- |
  | `#` | Which key picks it? | The row's place on screen: `1` at the top, up to `99` |
  | `STATE` | What needs me? | The kind when the row has one (`permission`, `question`), since the red glyph already says blocked; else the state word. A suspect row reads `idle?` |
  | `AGE` | How long has it been like this? | Time in state |
  | `TURN` | How long did its last turn take? | The last working turn, `-` before the first one ends |
  | `HARNESS` | Which agent? | `claude`, `codex`, ... |
  | `HOST` | Which machine? | The Rex host |
  | `WHERE` | Which pane? | `session › window › block`, until the split |
  | `SESSION` | Which session? | The Rex session |
  | `WINDOW` | Which window? | The Rex window (a tab in the Rex app) |
  | `BLOCK` | Which pane? | The block's label |
  | `MODEL` | Which model? | The model the harness names, from [`rexboard usage`](cli.md#rexboard-usage) |
  | `CTX` | How full is its context? | Percent of the context window in use: yellow from 70%, red from 85% |
  | `TOK` | How big was its last request? | The prompt side of the last model request, cache hits included: `950`, `68k`, `1.0M` |
  | `COST` | What has it cost? | The harness's own estimate for the session, at list price: `$4.20`, `$123` |
  | `5H` | How much of the quota is gone? | Percent of the five-hour quota used, where the plan has one; coloured as `CTX` |
  | `MSG` | What did it say? | The agent's own message: its first line with words that is not a heading, or the heading when that is all there is. Its inline markdown is drawn (see [Markdown](#markdown)), and any heading, quote or list marker it starts with is dropped. Blank when there is none, and for a hook's filler (`Done`, `Working`, `Allow <tool>?`); the turn card has the title. See [Agent status](agent-status.md) |

  What a row shows beyond its columns:

  - **State:** a glyph and colour per state (see [Style](#style)); working spins. Time in state is `≥5h43m` when estimated and `-` for a pane with no status record. A harness read from the title, with no record, ends in `?` (`claude?`).
  - **Suspect:** a `working` report more than 120 seconds old on a pane that looks idle (an idle `✳ ` title, or a shell back in front) is not trusted. The row shows `idle?` (from 100 columns; narrower, its `○` glyph), dim, under `UNREPORTED`, and the turn card says `may be stale:` and why. With `tmux`, or the `sh` wrapper around it, in front the title is tmux's, so only a shell back in front makes such a row suspect. serve checks every second, since time alone can make a report suspect.
  - **Down:** rows of a host that is down show `◌` and are dim, and so is every row while serve is stopped.
  - **Selection:** a raised background.
  - **Paths:** a pane labelled `shell`, or named after its foreground process, tells you nothing, so its path is `session › window` (also below 100 columns), and its `BLOCK` is its title, else its window. A path too long for its column is cut window first (to `…` below 6 columns), then the session down to an 8-column stem, then the last label.
  - **Twins:** rows on one host that would still read the same end in `#N`, their place among those rows by window and then by block age. A number can shift when a twin's title changes and it stops reading the same. Once `WHERE` is split, `#N` goes on `BLOCK`, and a long session, window or block keeps both ends with `…` in the middle. Below 60 columns, with no host column, a row from a host other than `local` also gets `@HOST` when a twin is on another host: at 50 columns, `api…way › pair #1`, `api…way › pair #2` and `ap…y › pair @mini`. The suffix is never cut; the path gives way first, down to `…`.
  - **Overflow:** when rows do not fit, the last line says how many are below (`↓ 4 more`).
- Nothing under the list by default: the list fills the body. `c` shows the **turn card** for the selected row, under the list at full width, at every width (see [Turn card](#turn-card)); `p` shows a **preview strip** above the footer instead, a bar and the last lines of the row's pane (see [Preview strip](#preview-strip)). With both on, the card wins.
- **Footer:** one of these:
  - the keys that fit, most needed first: `j/k move · enter jump · space full · p preview · ? keys · n needs you · / filter · m seen · a answer · c card · v next view · 1-99 select`, then `ctrl-e/y scroll card` while a card shows. A narrow board drops them from the end, so 80 columns ends at `n needs you`, and `q quit` sits at the right edge (`esc cancel` in `pick`). A kept filter starts it with `/mini · esc clears ·`;
  - a waiting prefix's next keys;
  - the filter being typed;
  - a message, for 3 seconds.

  A filter that matches nothing says `no rows match "mini" · esc clears` in the list.

Long labels are cut to fit: a session, window or block label to 24 columns with `…` in the middle (a label of ` · ` parts keeps those separators, and its longest part is cut inside), and every cell at its column's edge with `…`. Widths count CJK and emoji as two columns. Colours are ANSI named colours only, so the terminal's theme carries through.

## Style

One table in `src/tui/theme.rs` says how each state looks, and the list, the turn card's bar, the full view's bar and `pick` all draw from it. Every glyph is one cell wide in any terminal and never an emoji, and its shape says the state without the colour.

| Glyph | State | Look |
|---|---|---|
| `!` | blocked on a permission, or on anything but a question | red, bold |
| `?` | blocked on a question | red, bold |
| `✗` | error | red, bold |
| `✓` | done, not yet seen | green, bold |
| `✓` | done and seen | dim |
| `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` | working: one frame every 160 ms, from the board's own clock | yellow |
| `○` | idle, or a `working` report the board doubts (`idle?`) | dim |
| `·` | no status, or a state read from the title only | dim |
| `◌` | its host is down | dim, and so is the row |

The board redraws every 160 ms only while a working row is on screen: in the list, on the turn card's bar or on the full view's bar. Otherwise it keeps its once-a-second tick.

The rest of the board's text takes one of a few roles, also in `theme.rs`, so a new screen picks a role rather than a colour:

| Role | Look | Where |
|---|---|---|
| `alert` | red | a failed jump, read, close or copy, a host down, an extension failing, a refusal; answer mode's bar; the close question (bold) |
| `note` | cyan | a passing message (a toast), a jump, close or answer on its way |
| `accent` | cyan, bold | the board's name, the key table's title, an answer armed to send |
| `strong` / `quiet` | bold / dim | a key and the words after it in every footer; a headline and its labels |
| `header` | dim, bold | the column header and the group headers |
| `selected` | raised background | the selected row or group header, keeping each cell's own colour |
| cut mark | dim `…` | a pane line cut at the edge, on the line's own background |

The glyph keeps its state's look everywhere, on the bars too: a spinner drawn bold turns into a still blob in a font with no bold braille.

**Reduced motion** holds the spinner still as `⠿`, and the board keeps its once-a-second tick. Turn it on with `"motion": "reduced"` in `tui.json` (see [Turn card](#turn-card) for where the file lives), or for one run with `REXBOARD_REDUCED_MOTION=1`. The `?` table says so in its title when it fits.

## Views

A view is a way of grouping the rows plus a sort. The view and sort last for the run.

| View | Groups | Order inside a group, with no sort |
| --- | --- | --- |
| `status` (the default) | `NEEDS YOU`, `WORKING`, `QUIET`, `UNREPORTED`, as above | As above |
| `table` | None: one list with no group headers, sortable by any column with a key or a click | The `status` view's order |
| `harness` | One per harness (`CLAUDE`, `CODEX`), a to z, with `?` (no harness) last | The `status` view's order |
| `host` | One per host (`LOCAL`, `MINI`), a to z | The `status` view's order |
| One per extension field (`rig`, `project`) | One per value (`API`, `WEB`), a to z, with `-` (no value) last | The `status` view's order |

In a grouped view a blank row sits above each group header but the first, while every row fits; once the list would scroll they go, so they never cost a row. The selection never rests on one. A view grouped by a column leaves that column out, since its group header says it: the `harness` view has no `HARNESS`, the `host` view no `HOST`, and a field's view no column for that field. `MSG` gets the room.

`o` and a column's letter sorts by that column inside each group: `s` state, `a` age, `t` turn, `h` harness, `o` host, `w` where, `e` session, `n` window, `b` block, `d` model, `x` ctx, `k` tok, `c` cost, `5` 5h, `m` msg. The usage letters sort only while their column shows. An extension field takes the first letter of its name that no other column has (`rig` sorts with `r`, `project` with `p`). The header underlines each letter. `w` sorts by the whole `session › window › block`, and `e`, `n` and `b` by one part, whether or not `WHERE` is split. `o t` sorts by turn even when the `TURN` column is not drawn.

Sorting works like a table header: the first press sorts, the same letter again flips it, and a third press, or `esc`, clears it. The first press sorts `AGE` and `TURN` longest first (`↓`), `STATE` most urgent first (blocked, error, done, working, idle; `↑`) and the text columns a to z (`↑`). A row with no value (no status record to date it, no finished turn) sorts last either way. In the `status` view, `NEEDS YOU` keeps its blocked rows first under any sort. In the `host` view rows leave out `HOST` and `@HOST`, since their group names the host.

## Extension fields

Each field an enabled [extension](extensions.md) declares gets a view, a column and a filter key. The board learns them from serve, which sends them with the rows from the moment the extension is enabled, before its first run. With no extension enabled, the board is as it was.

- **View:** one per field, after `host` in the `v` cycle and open from `:` by the field's name. A field named like a built-in view (`host`) is opened by its id (`fake.host`). Its column is hidden there, since the group header shows the value.
- **Column:** after `HOST`, titled from the manifest (`RIG`), in `enabled` order and then each manifest's, from 120 columns; below 160 columns only the first two fields get one. A row with no value shows `-`, and sorts last.
- **Name:** the view, the filter key and the card use the field's `name`, and the column its `title`, so a manifest that keeps the title the name in capitals has a column that says what to type. A field goes by its short name (`rig`), unless two enabled extensions declare the same name; then both go by their id (`fake.rig`, `other.rig`), in views, filters and the card alike.
- **Keys:** while there are fields, `?` adds a row after `o` with their sort letters and names, and each extension when it fits: `o r p  sort by rig, project`, or `o r  sort by rig (fake)`.
- **Card:** every field of the selected row, one line per extension under the title and `source` line: `fake · rig api · owner ana`, or `lane desk` for an extension whose one field has its name. Fields without a column show here too.
- **Alert:** an extension whose last run failed shows in red in the top bar, as a host that is down does. `rexboard ext ls` says why.

The column header underlines each column's sort letter and puts `↑` or `↓` just left of the sorted column's name, or just after it when only one space separates it from the column before (`HARNESS HOST↑` at 100 columns). When the view is not `status`, or a sort is set, the top bar says so after `rexboard`: `view: table ↓age`. The selection stays on its row when the view or the sort changes.

In the `table` view a click on a column's header sorts by that column, as its letter does: once to sort, again to flip, a third time to clear. A click on the arrow acts as its column's letter: it flips the sort, or clears it if it is already flipped. A click on a row selects it, and the list keeps its place, so the row stays under the pointer. The wheel moves the selection as `k` and `j` do, but stops at the ends; over a card with more text than fits, it scrolls the card instead. While the `/`, `:` or `f` prompt is open, clicks and the wheel do nothing. Other clicks do nothing. The first time the view opens in a run, the footer says `click a header to sort · hold Option (iTerm2) or Shift to select text`. Reading clicks needs mouse capture, which stops the terminal's own text selection, so rexboard turns it on only while the `table` view shows and off as you leave it or quit, including on a signal or a panic. In other views, and in `pick`, text selection works as usual and sorting is by letter only. Many terminals still select text in the `table` view with a modifier held (Option in iTerm2, Shift in many others). `:flat` also opens it.

## Keys

The keys are vim's where vim has one, and plain keys or vim's own ctrl chords otherwise, so they work in a stock Rex window: Rex binds cmd chords and passes every other key to the pane. One table in `src/tui/keys.rs` drives what each key does, this `?` table and every footer. A prefix key (`g`, `o`, `z`, `y`, `f`, `d`) waits for the next one and the footer lists what may follow it, as `g… g top · t next view · T previous view · esc cancels`; `esc` or any other key drops it.

| Key | Does |
| --- | --- |
| `j` / `k`, arrows | Move between rows and folded groups; the ends wrap |
| `gg` / `G`, Home / End | First / last |
| `H` / `M` / `L` | The top, middle or bottom of what the screen shows |
| `ctrl-d` / `ctrl-u` | Half a screen down / up, stopping at the ends |
| `ctrl-f` / `ctrl-b` | A screen down / up |
| `]` / `[` | The next group's first row; the start of this group, then the one before |
| `n` / `N`, `tab` | The next / previous row that needs you, round the ends. Leaving a `done` row whose card showed for at least a second marks it seen; a blocked or erroring row stays as it is. `tab` is `n` |
| `1`-`9`, then maybe a second digit | Select the row with that number in the `#` column: the rows on screen, in every view, under a filter and in `pick`. A digit selects at once unless a two-digit number on screen starts with it (`1` when the screen has 10 rows or more, `2` when it has 20): then it waits 400 ms for a second digit, and the footer shows `1…`. `enter` ends the wait early with that row (it does not jump), `esc` cancels, and any other key ends the wait and then does its own job. A number with no row says which rows are on screen (`no row 19 · rows 1-15 on screen`). Before, these keys selected the nth `NEEDS YOU` row; `n`, `N` and `tab` still walk those |
| `enter` | Go to the pane in the Rex window you are in, on its host, and mark it seen, as [`rexboard jump`](cli.md#rexboard-jump-next-and-pick) does. The footer says `going to SESSION › BLOCK…` while it runs and a second jump waits for it |
| `-`, `ctrl-o` | Back to the pane before the last jump, as [`rexboard jump -`](cli.md#rexboard-jump-next-and-pick) does; again comes back, like `cd -`. The footer says `going back…`, then `back to SESSION › BLOCK`, or `previous pane is gone` |
| `/` | Filter rows by host, session, window, block, title, state, message or harness; `enter` keeps it, `esc` clears it. A `key:value` term matches one key: `host:mini`, `state:done`, or an extension field (`rig:api`). Terms are and-ed |
| `f`, then a letter | Show one status only (see [Status filter](#status-filter)); `ff` clears it |
| `:` | The command line: a view's name (tab completes it, `ta` to `table`; enter opens it, or the one view the text starts), `:seen` to mark every unseen `done` row the list shows seen (a kept filter narrows it; it says how many), or `:q` to quit. `esc` cancels. An unknown name says `no view "x"` |
| `v` / `V`, `gt` / `gT` | Next / previous view: `status`, `table`, `harness`, `host`, one per extension field, round again |
| `o`, then a column's letter | Sort by that column (see [Views](#views)): once to sort, again to flip, a third time to clear |
| `za` / `zo` / `zc` | Fold or open the selected row's group (`za` toggles; on a folded group's header it opens it) |
| `zR` / `zM` | Open every group / fold every group. `NEEDS YOU` opens by itself when a new row enters it |
| `p` | Show or hide the live preview strip above the footer (see [Preview strip](#preview-strip)); the choice is kept in `tui.json`. While the card is showing, the card wins and the strip stays hidden. `pick` has none and ignores `p` |
| `c` | Show or hide the turn card; it is off until you turn it on. The choice is kept in `tui.json`. The key table, answer mode or a failed jump's reason still open the card until they go. `pick` has no card and ignores `c` |
| `ctrl-e` / `ctrl-y` | Scroll the card's text a line down / up, under its bar; the mouse wheel over the card does the same in the table view. Moving to another row starts its card at the top (a blocked row's at its dialog) |
| `space` | Open the [full view](#full-view) of the selected row's pane, read again every 2 seconds while it is open |
| `a`, `i` | Answer the selected row from the board: pick a dialog's option or reply with a line. See [Answer mode](#answer-mode) |
| `m` | Mark the selected pane seen, through serve. The message names it `session › block` (`session › window` for a generic block label). A blocked or erroring row stays in `NEEDS YOU` |
| `dd` | Close the selected row's pane. The footer asks, in red, `close WHERE on HOST, ending HARNESS (STATE)? y closes`, with WHERE as the list tells twins apart (`api › main #2`) and the state when it is blocked or working, with its kind (`blocked · question`); a narrow footer drops `ending …`, then cuts WHERE in the middle, never the key. `dd` only asks when its first `d` comes on its own, 300 ms after the key before (else `keys came too fast; nothing closed · dd closes`). Only `y` closes it, with `rex block close`, and only once the question has been up for 300 ms: a key sooner says `nothing closed (keys came too fast)`, and any other key says `nothing closed`. Either way that key does nothing else. The question goes if its row leaves the board. A pane whose process is `tmux` adds `(tmux keeps running)`: closing it ends the tmux client, not the session. The board's own pane, and any pane running `rexboard`, are never closed. Not in `pick` |
| `x`, then an action's key | Start an action an [extension](extensions.md#actions) offers for the selected row, such as a message to its seat. See [Actions](#actions). Not in `pick` |
| `yy` / `yp` | Copy the selected row's message (`yy`), or where its pane is as `SESSION › WINDOW › BLOCK @HOST` (`yp`), to the clipboard of the machine the board runs on: `pbcopy` on macOS, `wl-copy` under Wayland, else `xclip`. Both go without escape sequences or control characters, so a paste cannot end early and run what follows. The footer says what it copied (`copied "Tests pass: 12 passed"`, `copied api › main › seat-b @local`), or why not (`could not copy: no clipboard to copy to`). Rex does not pass OSC 52 from a pane to the Mac's clipboard, so the board does not use it |
| `R` | Reconnect to serve |
| `?` | Show every key in the card, in as many columns as fit, a blank row above the footer; the list keeps every row above it. `?`, `q` or `esc` hides it |
| `esc` | Drop a waiting prefix; else close the key table; else clear the `/` filter, the status filter and the sort |
| `q`, `ctrl-c` | `q` closes the layer in front (the key table, the full view) and quits only from the bare board; `ctrl-c` always quits (in answer mode it only leaves it) |

**Moved keys.** For one release an old key does nothing but say where it went: `s` (`mark seen is m now`), `ctrl+s` (`:seen`), `S` `A` `T` `O` `W` `E` `B` (`sort by host is oo now`: `o` then the letter), `r` (`R`), `Y` (`yp`), `ctrl+k` (`dd`). `R`, `M` and `N` changed meaning (they sorted); the first press of each in a run also says so.

**Typing guard.** The board's pane often gets text meant for an agent, and typing comes faster than 300 ms a key. So `dd`, `a` and `i` only act when pressed on their own, 300 ms after the key before; `dd`'s question waits 300 ms before `y` counts; and answer mode leaves at once if its first key comes within 300 ms of opening (`keys came too fast; nothing sent · a answers`), and stays left: the rest of the typing lands on the board, where it can move the selection but not answer or close. A burst such as `added hidden`, or `please run tests` with a pause anywhere in it, sends nothing and closes nothing. One way through is left: typing that starts with `a`, `i` or `dd` and pauses right after it, which is what a person pressing the key does; the reply line then shows the text before `enter` sends it.

The board redraws when serve sends rows, on a key, and once a second for the clock and the times in state. SIGTERM and SIGHUP (closing its window) quit it the same way as `q`, so the terminal is left as it was, and the board exits on its own within a second if its terminal goes away.

## Turn card

The card is the selected row in full, under the list, as wide as the board:

```
━━ ✓ done · seat-a · ended 4m ago ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ captured 11:21:14 · 8s ━━
 Opened PR #31; CI is green.
   Pushed feat/turn-card and opened PR #31.
   Next step is yours: review and merge.

 where   api › main › seat-a
 host    mini     harness claude   turn    6m
 session-prefix project api
 title   seat-a   source  native
```

1. **The bar:** what you scan first. The glyph and the state (with its kind, as in `blocked · permission`), the block, and how the turn stands: `ended 4m ago` for a done row, `waiting 12s` for a blocked one, `failed 2m ago`, or `for 2m` in any other state. On the right, how fresh the pane's tail is: `captured 11:21:14 · 8s`, or `newer report · space refreshes` once the row has reported since.
2. **The headline**, in bold: the first sentence of the message's first paragraph, when it is 120 characters or fewer; a longer first sentence is shown plain, not as a heavy block. A paragraph with no sentence stop is one sentence, so a short one (`Ready for review`) is all bold and a long one is plain. A sentence ends at `.`, `!` or `?` followed by a space (a `.` also needs a capital letter after it, and no `e.g.`-style dot before it, so `e.g. the docs` and `v1. 2 files` do not end it). With no message, the title. See [Agent status](agent-status.md).
3. **The ask or the body:** the message's other lines, wrapped, whole.
4. A failed jump's reason, and `may be stale:` for a suspect row.
5. The pane's tail, when it has been read, newest line at the bottom.
6. **The facts**, after one blank line, at the foot of the card, as a grid. Each fact is a track: a dim label cell (7 columns in words, 1 with glyphs) and a value (20 columns), the tracks 3 columns apart, so a label and a value sit in the same column on every row. One track to a row below 59 columns of card (47 with glyphs), two from 59 (47) and three from 91 (73). `where` (`session › window › block`) sits alone on top. Then three groups, each starting a new row, in this order: how it runs (`turn`, how long the last turn ran; `model`; `ctx`, `88% · 176k of 200k`, the share, then the last request's tokens of the window; then `cost` and `5h` when `tui.json` asks for those columns), then each extension's fields on the row, then the pane (`title`, `source`). A group with more facts than tracks wraps, and its next row starts under the first track. A value cut at its track, or at the row's edge when the track would pass it, ends in `…`; alone on its row it may use the rest of the width. A fact with no value, or an empty one, is left out, and the facts after it close up.
7. **What the card leaves out**, because the list row already says it: anything a column drawn on screen already shows. `host` and `harness` while their columns are on screen (from 60 columns); `turn` while the list has a `TURN` column (from 140 columns); `where` while the list shows the session, window and block whole, in `SESSION`, `WINDOW` and `BLOCK` or in `WHERE` from 100 columns, and it comes back when the list had to cut one of the three or added a twin's `#N`, so a twin can still be told apart; an extension's field while its column is on screen (a field gets a column from 120 columns, the first two below 160); and a `title` that only repeats the block's label. A column that only sits past the right edge, or is hidden, as grouping by it does, is not on screen, and its fact is on the card. `rexboard show` and `--json` say everything and are unchanged. An extension's field is a plain fact named after the field, `rig` or `project`; when two enabled extensions declare the same field name, both go by their ids, cut in the middle to the label cell (`ope…rig`).

The card keeps one height for every row, so the list holds still while you move through it. Text longer than the card scrolls under the bar: a dim `↓ 12 more · ctrl-e` closes the card while there is more below, and `↑ 3 above · ctrl-y` opens it once it has scrolled, and `ctrl-e` and `ctrl-y` move it a line. A card opens at the top of its text. A blocked row's tail shows only the dialog, from the rule above it (with no dialog on screen, as much of the tail as fits), and its card opens scrolled to the dialog's last line, so the options show with as much of the dialog above them as fits; `↑` says when the dialog starts higher, and the facts sit below it. A row's tail otherwise takes the rows its message and facts leave.

A row with no status record says `no status from this pane` and `space captures the screen`.

The card is 40% of the body, and at least 8 rows, and more when the list is short: it takes the rows the list leaves, so it sits right under the list with no blank band between them (the list keeps its own rows, and the card shrinks back to 40% as the list grows). Or what `"card_height"` in `tui.json` says: a number of rows (`12`) or a share of the body (`"30%"`, at least 8 rows); any other value is the default. It always leaves the list 3 rows under its column header, and its last row, above the footer, stays blank. Its text starts under its bar, so the bar holds its row as the selection moves. The key table takes its own rows instead, and answer mode as many as its options need. Text wraps at 120 columns however wide the board. `c` hides it.

Reading a card does not mark a row seen. `m` does, and so does `n` or `tab` off a `done` row whose card has shown for at least a second. The tail of a blocked row is read by itself once its card has shown for 300 ms, once per report, so the ask can be checked against the screen; moving through rows reads none.

`c`'s choice lives in `tui.json` beside serve's `state.json` (`~/.local/state/rexboard/tui.json`, or under `XDG_STATE_HOME`), as `"card": "on"` or `"off"` (missing means off). Before this key a hidden card was `"detail_hidden": true` and a shown one the default: while `card` is missing the file's `detail_hidden` still says what it said (`false` is on, `true` is off), and a save writes `card` and keeps `detail_hidden` as it was. Next to it are `preview`, `motion` (see [Style](#style)), `icons` (see [Icons](#icons)), `card_height`, `columns` and `widths` (see [Layout](#layout)), which a save keeps as they were. The file is read when the board starts and written on each `c` or `p`. The side pane's old width key is ignored.

`"tail_lines"` sets how much of a pane's end the card and the full view keep: 50 to 5000 lines, 200 by default (anything else is the default). The most bytes a read asks for grows with it, from 96 KiB of coloured text and 48 KiB of plain text per 200 lines, so a 1000-line tail reads about 480 KiB in the full view. It costs one bigger read when the card or the full view opens, never one per row.

### Preview strip

The card and the preview are both off by default: `c` and `p` turn them on, and `space` opens the full view. `p` shows the selected row's pane above the footer, with the list above it:

```
── seat-a · working · for 2m ───────────────────────────── captured 11:21:14 · 1s ──
 │ running 12 tests
 │ test parser::nested ... ok
 │ test result: ok. 12 passed
 │ Allow Bash: cargo test? (y/n)
```

The bar is the card's: the row's state, its name, how long it has been so, and how fresh the tail is. Under it are the last `preview_lines` lines of the pane (1 to 12, 4 by default), wrapped to the board's width. The strip follows the selection: a row that stays selected for 300 ms is read, then again every `preview_every` seconds (1 to 10, 2 by default). Each read is one bounded Rex tail read of only those lines, never started while the last one is still out, and while the strip is hidden nothing is read. The lines already shown stay up between reads. A blank row separates the strip from the footer, as it does the card. The list always keeps its 3 least rows, so a short screen shows a shorter strip, or none, and then nothing is read for it; the choice stays.

The card and the strip are both inline detail, so only one shows: with `c` on, the card shows and `p` only says so (`the card is showing, so no preview`). `p` and `c` are kept separately in `tui.json`, so turning the card off brings the strip back. `pick` shows neither.

To set the strip, write `"preview": "on"`, `"preview_lines": 6` or `"preview_every": 5` in `tui.json`; an out-of-range number is the default, and a save keeps it as written.

### Icons

With `"icons": "nerd"` in `tui.json`, or `REXBOARD_ICONS=nerd` for one run, the card's facts take a Nerd Font glyph in place of each word label: the turn, the model, ctx, cost and 5h, the title, and how the status arrived (native, bridge or inferred). `where` leads with the session's mark and gives the window and the block their own: `<session> api › <window> main › <block> seat-a`. An extension's field takes the mark its manifest declares for it (`icon` on the field); one with no mark keeps its name, and a card with such a name keeps the words' grid for all its labels: the glyph labels of the other facts then sit in 7-column label cells, three tracks from 91 columns, not 73. The list's `HARNESS` cell leads with the harness's own mark (`<claude> claude`; Claude, Codex and pi each have one), and in the `harness` view the mark leads the group header (`<claude> CLAUDE (2)`); a harness without one keeps its word, lined up under the marked ones in the cell, and its group header starts at the left edge. A mark belongs to its harness adapter (`Harness::mark` in `src/harness/`), so a harness rexboard has no adapter for has none. `REXBOARD_ICONS=text` turns them off for a run; the environment wins, then `tui.json`, then words. Any other value is words.

Nerd mode needs a Nerd Font **Mono** variant from Nerd Fonts **3.5.0** or later: the Claude, OpenAI and inferred marks are new in 3.5.0, and a non-Mono font draws glyphs wider than the one cell the board gives them. The board never guesses the font, and shows no hint of its own: `rexboard hooks status` ends with `icons  nerd marks on`, or with two lines when they are off: `icons  off (words). Nerd Font? This should show one glyph between the bars:` and, indented under the label, `printf '|\356\257\222|\n'   then set "icons": "nerd" in tui.json`. One glyph between the bars means the terminal can draw them; set `"icons": "nerd"` in `tui.json` (`REXBOARD_ICONS=nerd` tries it for one run). Column headers, the bar, the list (but for the harness marks) and every other screen keep their words in both modes, and the state glyphs are the same. Every glyph is pinned by its Nerd Fonts name and code point, in `src/tui/icons.rs` or on its harness adapter, each a single code point in the Basic Multilingual Plane, one cell wide.

### Markdown

`MSG` and the card's message draw an agent's inline markdown: `**bold**`, `__bold__` and `` `code` `` in the `strong` role, and `[text](url)` as its text. The markers go, and the cut and the wrap count what shows, not the markers. Italics stay as written, so `snake_case` and `2*3*4` are unchanged, and an unmatched marker shows as typed. The card lays a message out by line:
- one blank line between paragraphs; a run of them is one, and none at the top or bottom;
- a heading loses its `#`s and is all `strong`;
- a list item wraps under its text, not its marker (`- ` hangs 2 cells, `12. ` 4), and a nested one keeps its depth. Past 9 a numbered list's numbers right-align (` 9.`, `10.`), so the text keeps one edge. A quote keeps `> ` on each row;
- the lines between code fences show as typed, 2 cells in, cut at the card's edge with `…` and never wrapped; the fence lines go, with their info strings. Code is not highlighted;
- a table's rows (lines that start with `|`) line up: each column is padded to its widest cell as it shows. The row above the `|---|` separator is the header and `strong`, and the separator goes. A row is cut the same way as code, never wrapped.

Blank lines and indentation reach the card only from reports `rexboard hook` made from 0.16; an older report is one paragraph. Control and escape sequences are removed first. Each report is mapped once, not on every frame. Sorting by `MSG` (`o m`, `ls --sort msg`) goes by what `MSG` shows, and `/` matches a phrase as it shows as well as as it was sent. `rexboard ls --json`, `rexboard show` and what `yy` copies keep the raw text.

## Full view

`space` gives the selected row's pane the whole terminal: a bar on top, the pane's lines in between, a blank row, and the keys at the bottom. There is no list, and no facts, so the bar is the card's with where the row is and its host added: `session › window` and the host give way, in that order from the host, when the bar is short of room, never the outcome.

```
━━ ⠋ working · seat-b · api › main · mini · for 2m ━━━━━━━━━━━━━━━━━━ live · 1s · following ━━
...the pane's last lines, newest at the bottom...
space back · j/k scroll · a answer · enter jump · gg/G top, bottom · ctrl-d/u half page · ctrl-f/b page
```

A narrow terminal drops whole keys from the end of the footer: 80 columns ends at `gg/G top, bottom`.

- **Lines:** the end of the pane as tail.lua reads it in Rex's `vt` format, with its colours: the last 96 KiB, the last 200 lines kept. A coloured line takes about twice the bytes of plain text (2x on a Claude Code pane, 2.5x on a coloured diff), so 96 KiB holds the 200 lines unless they average over about 490 bytes each. Each line keeps the pane's own layout at any width: a line longer than the terminal is cut at the edge and ends in a dim `…`, never wrapped, and nothing caps the width. `enter` shows it whole in the pane.
- **Colours:** the 16 terminal colours, the 256-colour palette and RGB, for text and background, and bold, dim, italic, underline, reverse and strikethrough, each until the pane resets it. Cursor-forward runs, which Rex writes for spaces, become spaces. Every other escape sequence and control character is dropped, since the text comes from whatever runs in the pane. The colours show as your terminal draws them: the 16 come from its palette. Spaces a program printed after its text never count as cut text: without a background they are dropped, and coloured padding, such as a diff row's, runs to the edge with no `…`. On a coloured row that is cut, the `…` takes the row's background, so the band runs unbroken to the edge. Rex restates a line's colours at its start, so a colour the pane set on one line and kept on the next shows on both. The turn card's tail and answer mode read plain text, and the card drops the colours of lines the view read.
- **Live:** while the view is open it reads the pane again every 2 seconds, counted from when the last read was asked, and asks again only once that read is back, so a slow host is never asked twice. Only the open row is read. Closing the view stops the reads, and the board's own tail of a blocked row waits while it is open. The bar says `live · 1s · following`, how long ago the last read came back.
- **Failed reads:** a host that does not answer in 3 seconds, or any failed read, leaves the last lines up and puts the reason and their age in the bar, `the pane did not answer within 3s · 2m old · retrying`. The reason is cut to half the bar, so the row's name keeps its room. With no lines yet, the bar says only `… · retrying` and the reason shows whole in red in the body.
- **Scroll:** the view follows the bottom. `k`, `up` or `ctrl-y`, `ctrl-u`, `ctrl-b` or `pgup`, `gg` or `home`, and the mouse wheel in the table view move up and pause it: the bar says `paused · G follows`, or `paused · newer below · G follows` once a read has brought lines the screen is not showing, and the lines hold still while reads go on. `j`, `down` or `ctrl-e`, `ctrl-d`, and `ctrl-f` or `pgdn` move back down, and reaching the bottom, or `G` or `end`, follows again with the newest read.
- **Keys:** `a` or `i` leaves for [answer mode](#answer-mode) on the row the view shows, and `enter` jumps to its pane, even when a filter or a folded group has moved the board's selection since; `space`, `esc` or `q` returns to the board, and `ctrl-c` quits. Other keys do nothing while the view is open. A jump's progress, a failed jump's reason and a passing message take the footer as they do on the board.
- The view closes by itself when its row leaves the board, and the footer says so: `seat-b left the board`.

## Answer mode

`a` (or `i`) answers the selected row without leaving the board. It opens only when pressed on its own, and a first key within 300 ms of it leaves again (`keys came too fast; nothing sent · a answers`), so typing meant for somewhere else neither picks nor fills the reply (see [Keys](#keys), Typing guard). The board first reads the pane, then the card turns into a red bar naming the target, the pane's last lines and a reply line:

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
| `1`-`9` | When the screen shows a numbered dialog (the reply line is then a dim `›` with no cursor), picks that option: one key press, no Enter. An option that grants something for later (`always`, `don't ask again`, `auto mode`, `this session`, `accept edits`) needs the same digit again within 3 seconds; the first press only says `press 2 again to send it`. A digit picks only an option the card shows: on a short board the card takes rows from the list, all of them if it must, and an option still off screen is refused (`option 1 is off screen; make the board taller or jump to it`) and left out of the footer's `1-3 pick` |
| text, then `enter` | When the screen shows an input line and no dialog, sends the line as a paste, then Enter. `enter` with nothing typed does nothing. Text into a dialog is refused: `pick a number; this dialog takes no text` |
| `esc` | Clears the typed text (the footer then says `esc clears`), or leaves when there is none |
| `ctrl-c` | Leaves; it never quits the board here |
| `tab` | Leaves and selects the next row that needs you; press `a` again there |

Board keys are off while answering, and nothing but these keys reaches the pane: never Esc, never Ctrl+C.

The mode is bound to the row's host and pane, not its place in the list, so a re-sort cannot move it. It is refused on the board's own pane and on a pane with no status record (`enter` jumps there instead).

Before anything is sent, one `rex do` on the pane's host checks, in order, that:

1. the agent still waits on the same thing: its status has the state and kind the board read. A fresh report of the same, such as the hook's notification a few seconds after a permission dialog opens, is no change. Else `the agent's status changed since it was read (blocked/permission → working); nothing sent`, naming what moved;
2. an agent, not a shell, is in front (else `zsh is in front, not the agent; nothing sent`); a tmux seat must still show the same seat's title. A seat behind the `sh -c ... tmux attach` wrapper OpenRig runs is not answered yet: `a seat behind OpenRig's sh wrapper around tmux cannot be answered from rexboard yet; nothing sent`. Jump to the pane instead (`enter`, or `rexboard jump KEY`);
3. the whole dialog, from its opening (below the rule above it) to its last option, or the input line and the line above it, is still on the last 40 lines, line for line. Every Edit dialog shows the same options, so the file or command above them is part of what must match (else `the screen changed since it was read; nothing sent`);
4. for a line, no dialog is up: options 1 and 2 with no input line below them (else `a dialog is up; pick a number; nothing sent`). Text into a dialog would be read as its highlighted option, so this holds whatever the board believes it shows.

On the board, `a` on a seat behind that wrapper says `a seat behind the tmux wrapper cannot be answered here yet · enter jumps` and does not open answer mode. A pane that is gone says `the pane is gone; nothing sent`. Only then does the key or the line go out, in the same script. A refusal sends nothing, clears any typed text, keeps the mode open with the reason in red, and reads the pane again. A read that shows a dialog clears typed text too.

Two failures are not refusals. `the pane did not answer within 5s, so it may or may not have the key; look before sending again` means the script ran out of time and the key may have gone out. `the text was pasted but Enter did not go through; it is in the pane's input line, so jump to it` means the paste landed and its Enter did not. Any other failure before the send ends `; nothing sent`. A line is also refused when the input line already holds typed text, which the reply would join (`the input line already has text, which a reply would join; nothing sent`). A dim placeholder or suggested prompt does not count as typed.

After a send the board returns to the same row, marks it seen, drops the pane's tail read before the answer (it shows the question just answered), and the card's bar says what went where: `sent 1 (Yes) → seat-a 11:22:04`, until the row needs you again with a newer report. Nothing moves on by itself; `tab` goes to the next row that needs you.

Options are read the same way for every harness: numbered `1.` or `1)`, with `❯` or `>` before the highlighted one, as Claude Code draws its permission prompts and questions. The board and `send.lua` read the screen by one copy of that grammar (`src/harness/dialog.rs`). A line reply goes to an input line any harness shows; a dialog drawn another way offers nothing to send, so jump to it.

## Actions

`x` lists the keys of the actions enabled [extensions](extensions.md#actions) offer for the selected row (`x… s send message · esc cancels`); with none it says `no action for this row`. An action's key opens one input line, which names the target:

```
Send message web › seats › seat-b on mini: ready for review?▏ · enter · esc
```

The end of what you type stays in view, and the head gives way when the screen is narrow. `enter` goes on, `esc` cancels, and every other key is text: `q`, `a`, `dd` and `x` do nothing else here. The line is red and is the board's, not the extension's:

```
send to seat-b@web on mini: "ready for review?" · it waits on a dialog · y runs
```

It is the extension's words for the target (`confirm`), then the text in quotes, whatever the extension's `confirm` says. When the row is `blocked` it adds `· it waits on a dialog`, and when it errored `· it reported an error`: text sent to a pane that waits on a dialog may be read as the dialog's answer, so the line says so before you press `y`. Only `y` runs it, and only once the line has been up for 300 ms; any other key says `NAME cancelled`. At `y` the board builds the plan again from the extension's current offer and the row as it is now; if it differs from the one shown, or the action or the row is gone, nothing runs and it says why. When the window is narrow the note gives way first (to `· dialog`, then to nothing), then the target is cut, then a long text, and the target and the text are always there. If no line with both fits, `enter` says `Send message cancelled: the screen is too narrow to confirm it` and nothing is armed; a window that narrows after the line is drawn refuses `y` the same way. A row that leaves, or stops taking the action, while you type or before you confirm ends the action with `NAME cancelled: that row is gone` in red.

The footer says `NAME…` while it runs and `NAME: FIRST LINE` after (`done` when the action said nothing). A failure shows `NAME failed: FIRST LINE`, with its exit code, in red until the next key:

```
Send message failed: exited with 3: not delivered
```

A second run waits for a jump or close in flight, and a running action is stopped when the board exits.

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

A row in `UNREPORTED` matches only `u`, so each row matches exactly one letter. While `f` waits for its letter, the footer lists each status with how many rows it would show, its letter underlined as in the column header: `f working 3 · done 5 · … · ff clears`. Narrower, the hint goes and the separators tighten, then it reads `f w3 d5 … · ff clears`. `ff` clears the filter, and `esc` drops the waiting `f`, leaving the filter as it was. Another letter leaves the filter as it was and says which letters `f` takes; a key that is not a character (an arrow, `enter`) does the same without a word. A key with `ctrl` or `alt` keeps its own meaning, so `ctrl+c` still quits. When nothing has the status, the list says `no error rows · ff or esc clears` (`no error rows match "x"` with a `/` text too). The top bar shows the filter after `rexboard` (`only question`, or `view: table ↓age · only question`).

The filter combines with the `/` text and with every view. Setting it opens folded groups, so the rows asked for are not hidden. The `#` numbers, `1`-`99` and `:seen` count only the rows the filters show. It works in `pick` too, where a plain `esc` exits as usual and `ff` clears the filter.

## Pick

`rexboard pick` shows the same list, with the same column header, without the turn card, at every width. Enter goes to the selected pane and exits 0; esc exits 1. `/` filters, as on the board.
