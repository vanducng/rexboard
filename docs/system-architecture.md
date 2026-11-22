# System architecture

rexboard reads Rex, remembers what Rex does not, and shows it. What it writes:

- **To a pane you chose, through Rex:** a jump focuses the pane and asks the Rex app to show its session; an answer, from the board ([Answer mode](tui.md#answer-mode)) or `rexboard answer`, checks the pane, then sends it a digit, or a line and Enter; a close, from the board (`dd`) or `rexboard close`, closes the pane.
- **To an agent's own terminal, from its hooks:** `rexboard hook` and `rexboard usage` print the status and usage records Rex keeps on that pane.
- **To your files:** its state dir ([Files](#files)), and, only when you run `rexboard hooks install`, the settings of the agents it names, after a backup.
- **Through an extension action:** the extension's own command, after your `y` ([Actions](tui.md#actions)).

<img alt="rexboard architecture: agents (Claude Code, Codex, pi, grok) report their state to Rex, on their own in a Rex pane or through a hook inside tmux. On this host and every host in rex hosts, Rex runs observer.lua, which streams NDJSON to rexboard serve; serve keeps state on disk, holds the harness adapters, runs the extensions (rows in, fields out) and answers the board, pick and the CLI over a unix socket. Three things write to Rex, each on one pane you chose, from the board or the CLI: a jump, an answer and a close act on the pane's host, and a jump then asks the Rex app to show the pane" src="assets/architecture.svg" width="100%">

Each host runs one observer, `rex [-S HOST] do - --all`, whose NDJSON lines ([CLI](cli.md#lines---raw)) feed serve's collectors and its one reducer thread. Some reads skip serve: the turn card's tail and the full view run `tail.lua` through `rex [-S HOST] do -` straight against the pane's host, `ls --direct` takes its own snapshot, and `watch --raw` and `watch --direct` run their own observers.

The image follows the reader's light or dark setting. Its source is [assets/architecture.html](assets/architecture.html); after editing it, copy its `<svg>` element out:

```bash
sed -n '/<svg /,/<\/svg>/p' docs/assets/architecture.html > docs/assets/architecture.svg
```

## Stack

| Layer | What |
| --- | --- |
| Rust | One binary, `rexboard`, in three roles: the CLI; `rexboard serve`, with one collector per host and one reducer thread that keeps state on disk; and the board and `pick`, a TUI on ratatui with its crossterm backend |
| Lua, in Rex | Scripts built into the binary that Rex runs through `rex [-S HOST] do -`: `observer.lua` for serve's observers, `snapshot.lua` for `ls --direct`, `jump.lua` after `app.lua` for a jump, `tail.lua` for a pane's tail, and `send.lua` after `tail.lua` for an answer. `lib.lua` holds the snapshot code `observer.lua` and `snapshot.lua` share |
| Rex API | The calls below. A jump, an answer and a close write |
| Observer contract | NDJSON, one JSON object per line with `"v": 1`, defined in `src/event.rs` |
| State | `serve.lock`, `serve.sock`, `restart.lock`, `state.json`, `transitions.jsonl`, `favorites.json`, `usage/`, `serve.log`, `tui.json`, `pick.json` and `board.json`, in the state dir (see [Files](#files)) |

rexboard reaches Rex only through the `rex` command: `rex do` for the scripts, `rex hosts --json` for the hosts, and `rex block close` for a close. It never opens Rex's socket itself.

| Kind | Rex API calls | Where |
| --- | --- | --- |
| Read | `server.version`, `session.attach_all` with `event_scope = "session"`, `session.list`, `session.view`, `session.list_blocks`, and on each terminal block `program_status`, `title` and `process` | `lib.lua`, `observer.lua`, `snapshot.lua` |
| Events | `block_event` (`program_status_changed`, `program_status_removed`, `title_changed`, `process_changed`, `child_exited`), `block_created`, `session_view_changed` (compared with what was announced, since Rex has no event for a move or a detach), `block_closed`, `window_created`, `window_closed`, `session_label_changed`, `window_label_changed`, `active_window_changed`, `session_created` and `session_destroyed`. `observer.lua` hears every session because serve runs it with `rex do - --all` | `observer.lua` |
| Read, for a jump | `client.list`, `server.whoami` for a remote host, and `session.attach` on the pane's session, which `session.focus_block` needs | `app.lua`, `jump.lua` |
| Read a pane's tail | `session.attach` on the pane's session (block methods answer only on an attached stream), then on the block `format` with `unwrap = true` and `max_bytes = 1` for the size and a blob id, and `blob_read` of the last bytes. Rex renders the whole scrollback on its side, and only those bytes cross the wire: 48 KiB of `text` for the card, 96 KiB of `vt` (with colours) for the full view | `tail.lua` |
| Write, a jump | `session.focus_block` on the pane's host, then `client.perform` with `action = "session.select"` on this Mac's Rex app | `jump.lua` |
| Read, for an answer | The tail as above, and the last 4 KiB again as `vt`, whose styles tell a typed draft from a dim placeholder | `tail.lua` |
| Write, an answer | On the pane's host, after it re-checks `program_status`, `process` (and `title` for a tmux seat) and the tail: `block.event` with a `key` event (`{"key": "digit_1", "action": "press", "text": "1"}`, no Enter) for an option, or a `paste` event and then a `key` event `{"key": "enter", "action": "press"}` for a line. Never Esc or Ctrl+C | `send.lua` |
| Write, a close | `rex [-S HOST] block close BLOCK` | `send.rs` |

## Pieces

Rust, under `src/`:

| Module | Does |
| --- | --- |
| `src/main.rs`, `src/lib.rs` | `main.rs` parses the command line and reports errors; `lib.rs` dispatches each command |
| `src/cli.rs` | The clap surface: every command and flag. Help text is interface |
| `src/error.rs` | The error type: a code, a message and `next_steps`, printed as JSON with `--json` |
| `src/config.rs` | `config.toml` |
| `src/marks.rs` | `[[mark]]` rules: the glob match, and which rows a rule marks. serve, `ls` and `watch --json --direct` apply them to the rows ([18](decisions/0018-rows-can-be-marked.md)) |
| `src/favorites.rs` | `favorites.json`: the panes marked with `*`, by their labels; serve reads it, changes it on `favorite` requests, and adds `favorite` to `marked_by` |
| `src/rex.rs` | Finds `rex`, runs one-shot scripts, and holds the one lock around every spawn, bind, accept and connect |
| `src/event.rs` | The observer line contract, `v` 1 |
| `src/collector.rs` | Keeps one observer alive per host, with backoff, in its own process group |
| `src/pane.rs` | One pane from Rex's records: the main record, `source`, the harness, the title glyph, the `turn` message |
| `src/board.rs` | The reducer: rows with `since`, turns, `seen` and groups, from host events and an injected clock |
| `src/serve.rs` | The daemon: collectors, the reducer thread, the socket |
| `src/state.rs` | Paths, the `flock`, `state.json`, `transitions.jsonl`, and the startup sweep |
| `src/client.rs` | `ls`, `watch --json`, `seen` and `serve` as clients; starts serve when nothing answers, and replaces an older one |
| `src/output.rs` | Text for the `ls` table and the board: labels and paths cut to fit, `#N` for twins, widths, ages |
| `src/show.rs` | `show`, `views` and `hosts`: what the board's card, `v` and top bar show, for the CLI; `show --tail` reads the pane through the board's own tail |
| `src/jump.rs` | `jump`, `next` and `pick`'s jump: runs `jump.lua`, then marks the row seen through serve; turns Rex's failures into plain errors |
| `src/send.rs` | Runs `send.lua` for an answer, within 5s, and turns its refusals into plain reasons; runs `rex block close` for a close |
| `src/act.rs` | `rexboard answer` and `close`: the same checks as the board's answer mode and `dd` before anything reaches a pane |
| `src/upgrade.rs` | `rexboard upgrade`: finds how this binary was installed and upgrades it the same way |

The harnesses and hooks:

| Module | Does |
| --- | --- |
| `src/harness/` | The agent harnesses rexboard knows, one file each (`claude.rs`, `codex.rs`, `pi.rs`, `grok.rs`) behind one `Harness` trait whose methods default to the shared behaviour: how a pane is recognised as one (its record's app, else the process in front, else the title) |
| `src/harness/dialog.rs` | The one grammar the board and `send.lua` read a dialog by |
| `src/harness/usage.rs` | `rexboard usage`: a harness's per-turn JSON becomes one usage file per pane. serve scans them every 2 s off the reducer thread and puts `usage` on its local rows ([Usage on every host](agent-status.md#usage-on-every-host)) |
| `src/hook/` | `rexboard hook`: `report.rs` turns a hook event into a status and its OSC 7501 sequence, pinned by `tests/fixtures/hooks/corpus.jsonl`; `mod.rs` finds the agent's tmux pane or tty and writes it there |
| `src/hook/install.rs`, `pi-extension.js` | `rexboard hooks install`, `uninstall`, `status` and `replay`, which merge the hooks into each harness's settings; and the pi extension it writes |

Extensions:

| Module | Does |
| --- | --- |
| `src/extension/mod.rs`, `contract.rs` | Discovery and manifests; the contract 1 request and response with its limits |
| `src/extension/runner.rs`, `running.rs` | One thread per enabled extension with its timeout and last-good rule; serve's set of them. `board.rs` takes their fields and fallback status |
| `src/extension/action.rs`, `cli.rs` | An action's argv template, prompt checks and run; `rexboard ext`, which builds `ext new` from `examples/extensions/session-prefix`. No tool is named in `src/` |

Lua, run by Rex through `rex do -`:

| File | Does |
| --- | --- |
| `src/lua/lib.lua` | The snapshot code `observer.lua` and `snapshot.lua` share |
| `src/lua/observer.lua` | Runs inside `rex do - --all --fail-fast`: one snapshot, then one line per event, and a `tick` every 30s |
| `src/lua/snapshot.lua` | One snapshot, for `ls --direct` |
| `src/lua/app.lua`, `jump.lua` | `app.lua` finds this Mac's Rex app among a server's clients; `jump.lua` attaches to the pane's session, focuses the pane on its own server, and asks the app to select the session |
| `src/lua/tail.lua` | Defines `tail(a)`: the last `a.max_bytes` of a pane, starting at a whole line, through `format` and `blob_read`, as `text` or `vt` |
| `src/lua/send.lua` | After `tail.lua`: re-checks that the pane still shows the report, the agent and the dialog or input line the board read, then sends; a refusal sends nothing |

The board and `pick`, under `src/tui/`, a client of serve's rows:

| Module | Does |
| --- | --- |
| `mod.rs`, `effects.rs` | The terminal and the loop; the board's effects and their threads: reads of panes, jumps, closes, answers and seen, and serve's rows as they change |
| `app/mod.rs` | The `App` and its dispatch, `act` |
| `app/filter.rs`, `keeping.rs`, `select.rs`, `views.rs` | The `/` text and `f` status; kept groups (`+`, `f+`, `:unkeep`); where the selection rests and how it moves, folds and clicks; views, the `:` line, sorting and extension fields |
| `app/pane_keys.rs`, `actions.rs`, `answer.rs`, `full.rs`, `card.rs` | `enter` and `-` jump, `dd` close, `yy` and `yp` copy; extension actions; answer mode on the board; the full view's state and keys; whether the card shows and the tail it reads. `testkit.rs` holds the tests' shared fixtures |
| `keys.rs` | The keymap: one table per layer (the board, the full view) of key sequences, what they do, and how `?` and the footers name them. Tests check that no key is bound twice and every key does something |
| `query.rs` | The views, `/` filter, `f` status, `+` kept groups and `o` sort run headless for `ls --view/--filter/--status/--sort/--keep`, `seen --all-done` and the board's launch flags |
| `columns.rs`, `views.rs`, `status.rs`, `keep.rs` | Which columns a width shows; views (a grouping plus a sort); the `f` status filter; the kept groups of each grouping |
| `view.rs` | Draws the board: the top bar, the list, the footer and the `?` table |
| `card.rs`, `meta.rs` | The turn card: bar, headline, ask or body, tail; and its facts at the foot, each once under a label, in aligned columns |
| `markdown.rs` | The inline markdown an agent's message carries, drawn as plain and strong runs |
| `full.rs`, `vt.rs` | The full view, read again every 2s; `vt.rs` keeps a line's colours (SGR) as styles |
| `answer.rs`, `screen.rs` | Answer mode: its keys and card, and what it reads off a pane (options, the input line, a draft) |
| `clipboard.rs` | `yy` and `yp`: `pbcopy`, `wl-copy` or `xclip` |
| `icons.rs` | The Nerd Font glyphs the facts take with `"icons": "nerd"`, pinned by name and code point |
| `theme.rs` | The one style system: the roles text takes and `face`, how each row state looks. Nothing else in `src/tui/` picks a colour or a glyph, and a test checks it |
| `prefs.rs`, `kept.rs` | `tui.json`; `pick.json` and `board.json`: load them, and save the keys a user changed |

serve uses std threads and channels, with no async runtime. Collectors, extension runners, the socket listener and each connection run on their own thread, and only the reducer thread touches the board. A runner sends its results to the reducer and never waits on it, so a slow adapter cannot hold up rows.

## Files

| Path | What |
| --- | --- |
| `$XDG_STATE_HOME/rexboard/` (default `~/.local/state/rexboard/`) | The state dir, mode 0700 |
| `serve.lock` | The `flock` that keeps serve to one instance, plus `{pid, rex, children}`: serve's pid, the `rex` path and its observers' pids |
| `serve.sock`, beside `serve.lock` | The socket, mode 0600. It depends only on the state dir, not on `TMPDIR` or `XDG_RUNTIME_DIR`, so every caller finds the one serve. A path over 103 bytes fails with `SERVE` (set a shorter `XDG_STATE_HOME`). A client refuses a socket served by another uid |
| `restart.lock` | Held by the one command replacing an older serve, so commands that find it at once restart it only once |
| `state.json` | `{"v":1,"panes":{KEY:{state,kind,since,since_estimated,turn_started,last_turn,seen}},"jumps":[KEY,KEY]}`, where `jumps` holds the last two jumps, older first, for `jump -`. Written through a temp file and a rename at most once a second, and on stop. Entries for a host that has not sent its first snapshot this run are kept as loaded |
| `transitions.jsonl` | One line per state change: `{at, host, block_id, from, to, kind}`. Over 5 MB at serve start, it moves to `transitions.jsonl.1` |
| `transitions.jsonl.1` | The previous `transitions.jsonl`, replaced each time the file passes 5 MB |
| `usage/` | One `KEY.json` per pane, `{key, ts, harness, model?, effort?, ctx_pct?, ctx_window?, tok?, cost_usd?, quota_5h?}`, written by `rexboard usage` through a temp file and a rename. `KEY` is `REX_BLOCK`, or `tmux:SOCKET:SERVER_PID:PANE` inside tmux, with any character but letters, digits, `.`, `_` and `-` made `-` |
| `usage/KEY.lock` | The `flock` the callers for one pane take to decide and publish a report once. It goes with its pane's file |
| `serve.log` | serve's output when a command started it. A client clears it first when it is over 1 MB |
| `tui.json` | The board's display settings on this machine, written by the board and `board --once`; [TUI](tui.md#tuijson) lists the keys. A missing or unreadable file means the defaults |
| `pick.json` | `rexboard pick`'s own view and sort ([17](decisions/0017-pick-keeps-its-view-and-sort-in-its-own-file.md)), written on a change of either; the board never reads it |
| `board.json` | The board's and `board --once`'s view, sort, status, `/` text and kept groups ([19](decisions/0019-the-board-opens-where-it-was-left.md)); [TUI](tui.md#boardjson) has the format and the save rules |
| `favorites.json` | The panes the user marked with `*`, `{"favorites": [{"host": H, "session": S, "tab": T, "pane": P, "seen": UNIX_SECONDS}]}`, by label and never by block id ([18](decisions/0018-rows-can-be-marked.md)). Only serve writes it, through a temp file and a rename (see below) |

serve scans `usage/` every 2 s off the reducer thread. It reads a file again only when its modified time changes, keeps a live pane's at any age, and removes one no local pane names once it is 10 minutes old. It joins a tmux pane through its client's pid (`ps`, `tmux list-clients`; the client is the pane's foreground process, or the tmux child of the wrapper that is, as under an `sh -c ... tmux attach`), and counts every pane on those tmux servers as live (`tmux list-panes -a`). [Usage on every host](agent-status.md#usage-on-every-host) says how the figures reach another host.

serve reads `favorites.json` again before each change and applies the one change. An entry unseen for 30 days goes at the next save, and serve refreshes `seen` on each rescan for the panes on the board, writing at most once a day. A file it cannot read means no favorites and is never overwritten.

On start, serve takes the lock, removes a stale socket, then stops observers a dead serve left behind. A recorded pid is stopped only if it is still its own process group, its parent is init or the recorded serve, and its command is the recorded `rex` running `do - --all --fail-fast`. Anything else is skipped and logged, because the pid may now belong to another program.

## Protocol

One JSON request per line on the socket; every reply line carries `"v": 1`.

| Request | Reply |
| --- | --- |
| `{"cmd":"rows"}` | `{"v":1,"rows":[ROW...],"hosts":{HOST:STATE},"host_errors":{HOST:WHY},"extensions":{...},"fields":[...]}`, with `config_problem` (see below). Right after serve starts, the reply waits up to 5s for every host's first snapshot |
| `{"cmd":"subscribe"}` | The same `rows` line now and on every change (at most every 100ms), plus `{"v":1,"heartbeat":true}` every second. Each subscriber has room for 4 lines; a reader that falls behind misses lines rather than growing serve, and the next `rows` line is a full snapshot again |
| `{"cmd":"seen","key":"HOST/BLOCK_ID","jump":false}` | `{"v":1,"ok":true,"key":KEY}`. `jump: true` (from a jump) also makes it where `next` without `after` starts |
| `{"cmd":"next","after":KEY,"back":false,"marked":false,"source":null}` | `{"v":1,"row":ROW}`: the next `needs_you` row after `after` (a key or a block id; else the last jump's pane; else the first), unseen rows first and all of them once every row is seen, wrapping; `back` goes the other way. With `marked: true` it walks the marked rows instead, in board order, seen or not, and answers `NO_MARKED_ROW` when none is; `source` (a name in `marked_by`, or `any`) keeps only the rows that source marked |
| `{"cmd":"favorite","host":H,"session":S,"tab":T,"pane":P,"on":true}` | `{"v":1,"ok":true,"favorite":true,"still":[SOURCE...]}`: `*`. serve makes the pane with these labels a favorite (`on`) or not, in `favorites.json`, and answers with the other sources that still mark it. `USAGE` for an empty pane label, `ERROR` when the file cannot be read or saved |
| `{"cmd":"back"}` | `{"v":1,"row":ROW}`: the pane the jump before the last one went to, for `jump -`; else `NOT_FOUND`, `no previous jump yet` or `previous pane is gone` |
| `{"cmd":"stop"}` | `{"v":1,"ok":true}`, then serve saves and exits |
| `{"cmd":"version"}` | `{"v":1,"rexboard":"0.9.0"}`, answered at once. Every client asks first and replaces a serve older than itself. A serve that answers `USAGE` predates the question, so it counts as older |

In the `rows` reply, `STATE` is `starting`, `up`, `down` or `stopped`, and `host_errors` says why for a host that is down or stopped, until it is up again. `config_problem` is `{short, why, last_good}` while serve cannot read `config.toml` (`short` is the top bar's `config.toml bad: line 12`, `why` the warning `ls` prints, `last_good` whether serve had read a good file, so it keeps those settings, or runs on defaults), else `null`; serve keeps its last good `[hosts] exclude`, models and mark rules meanwhile and logs the problem once. `extensions` is each extension's last run, and `fields` the declared fields (`{id, name, title}`) in `enabled` order, sent before any run so the board can show them.

Errors are `{"v":1,"error":{"code":CODE,"message":...}}`, with `CODE` one of `USAGE`, `NOT_FOUND`, `NOBODY_NEEDS_YOU` and `NO_MARKED_ROW`. `ROW` is the object `ls --json` prints.
