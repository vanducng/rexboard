# System architecture

rexboard reads Rex, remembers what Rex does not, and shows it. It writes to Rex in three places only, each on one pane you chose: a jump focuses the pane and asks the Rex app to show its session; an answer, from the board ([Answer mode](tui.md#answer-mode)) or `rexboard answer`, checks the pane, then sends it a digit, or a line and Enter; a close, from the board (`dd`) or `rexboard close`, closes the pane.

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
| State | `serve.lock`, `serve.sock`, `restart.lock`, `state.json`, `transitions.jsonl` and `tui.json`, in the state dir (see [Files](#files)) |

rexboard reaches Rex only through the `rex` command: `rex do` for the scripts, `rex hosts --json` for the hosts, and `rex block close` for a close. It never opens Rex's socket itself.

| Kind | Rex API calls | Where |
| --- | --- | --- |
| Read | `server.version`, `session.attach_all` with `event_scope = "session"`, `session.list`, `session.view`, `session.list_blocks`, and on each terminal block `program_status`, `title` and `process` | `lib.lua`, `observer.lua`, `snapshot.lua` |
| Events | `block_event` (`program_status_changed`, `program_status_removed`, `title_changed`, `process_changed`, `child_exited`), `block_created`, `block_closed`, `window_created`, `window_closed`, `session_label_changed`, `window_label_changed`, `active_window_changed`, `session_created` and `session_destroyed`. `observer.lua` hears every session because serve runs it with `rex do - --all` | `observer.lua` |
| Read, for a jump | `client.list`, `server.whoami` for a remote host, and `session.attach` on the pane's session, which `session.focus_block` needs | `app.lua`, `jump.lua` |
| Read a pane's tail | `session.attach` on the pane's session (block methods answer only on an attached stream), then on the block `format` with `unwrap = true` and `max_bytes = 1` for the size and a blob id, and `blob_read` of the last bytes. Rex renders the whole scrollback on its side, and only those bytes cross the wire: 48 KiB of `text` for the card, 96 KiB of `vt` (with colours) for the full view | `tail.lua` |
| Write, a jump | `session.focus_block` on the pane's host, then `client.perform` with `action = "session.select"` on this Mac's Rex app | `jump.lua` |
| Read, for an answer | The tail as above, and the last 4 KiB again as `vt`, whose styles tell a typed draft from a dim placeholder | `tail.lua` |
| Write, an answer | On the pane's host, after it re-checks `program_status`, `process` (and `title` for a tmux seat) and the tail: `block.event` with a `key` event (`{"key": "digit_1", "action": "press", "text": "1"}`, no Enter) for an option, or a `paste` event and then a `key` event `{"key": "enter", "action": "press"}` for a line. Never Esc or Ctrl+C | `send.lua` |
| Write, a close | `rex [-S HOST] block close BLOCK` | `send.rs` |

## Pieces

| Module | Does |
| --- | --- |
| `src/cli.rs` | The clap surface: every command and flag. Help text is interface |
| `src/lua/observer.lua` | Runs inside `rex do - --all --fail-fast`: one snapshot, then one line per event, and a `tick` every 30s |
| `src/collector.rs` | Keeps one observer alive per host, with backoff, in its own process group |
| `src/event.rs` | The observer line contract, `v` 1 |
| `src/harness/` | The agent harnesses rexboard knows, one file each (`claude.rs`, `codex.rs`, `pi.rs`, `grok.rs`) behind one `Harness` trait whose methods default to the shared behaviour: how a pane is recognised as one (its record's app, else the process in front, else the title), and `dialog.rs`, the one grammar the board and `send.lua` read a dialog by |
| `src/pane.rs` | One pane from Rex's records: the main record, `source`, the harness, the title glyph, the `turn` message |
| `src/board.rs` | The reducer: rows with `since`, turns, `seen` and groups, from host events and an injected clock |
| `src/serve.rs` | The daemon: collectors, the reducer thread, the socket |
| `src/state.rs` | Paths, the `flock`, `state.json`, `transitions.jsonl`, and the startup sweep |
| `src/client.rs` | `ls`, `watch --json`, `seen` and `serve` as clients; starts serve when nothing answers, and replaces an older one |
| `src/output.rs` | Text for the `ls` table and the board: labels and paths cut to fit, `#N` for twins, widths, ages |
| `src/rex.rs` | Finds `rex`, runs one-shot scripts, and holds the one lock around every spawn, bind, accept and connect |
| `src/jump.rs` | `jump`, `next` and `pick`'s jump: runs `jump.lua`, then marks the row seen through serve; turns Rex's failures into plain errors |
| `src/show.rs` | `show`, `views` and `hosts`: what the board's card, `v` and top bar show, for the CLI; `show --tail` reads the pane through the board's own tail, and `hosts` reads serve's host states and their last errors |
| `src/send.rs` | Runs `send.lua` for an answer, within 5s, and turns its refusals into plain reasons; runs `rex block close` for a close |
| `src/upgrade.rs` | `rexboard upgrade`: finds how this binary was installed and upgrades it the same way |
| `src/config.rs` | `config.toml` |
| `src/extension/` | Extensions: discovery and manifests (`mod.rs`), the contract 1 request and response with its limits (`contract.rs`), one thread per enabled extension with its timeout and last-good rule (`runner.rs`), serve's set of them (`running.rs`), and `rexboard ext` (`cli.rs`), which builds `ext new` from `examples/extensions/session-prefix`. `board.rs` takes their fields and fallback status. No tool is named in `src/` |
| `src/lua/tail.lua` | Defines `tail(a)`: the last `a.max_bytes` of a pane, starting at a whole line, through `format` and `blob_read`, as `text` or `vt` |
| `src/lua/jump.lua` | Finds this Mac's Rex app, attaches to the pane's session, focuses the pane on its own server, and asks the app to select the session |
| `src/lua/send.lua` | After `tail.lua`: re-checks that the pane still shows the report, the agent and the dialog or input line the board read, then sends; a refusal sends nothing |
| `src/tui/` | The board and `pick`, a client of serve's rows. `mod.rs` keeps the terminal and the loop |
| `src/tui/effects.rs` | The board's effects and their threads: reads of panes, jumps, closes, answers and seen, the messages they send back, and serve's rows as they change |
| `src/tui/app/` | The board's state and keys, one file per job: `mod.rs` (the `App` and its dispatch, `act`), `filter.rs` (the `/` text and `f` status), `select.rs` (where the selection rests and how it moves, folds, clicks), `views.rs` (views, the `:` line, sorting and extension fields), `pane_keys.rs` (`enter` and `-` jump, `dd` close, `yy` and `yp` copy), `full.rs` (the full view's state and keys), `answer.rs` (answer mode on the board), `card.rs` (whether the card shows and the tail it reads), and `testkit.rs` (the tests' shared fixtures) |
| `src/tui/keys.rs` | The keymap: one table per layer (the board, the full view) of key sequences, what they do, and how `?` and the footers name them. Dispatch, `?` and every footer read it, and tests check that no key is bound twice and every key does something |
| `src/tui/query.rs` | The board's views, `/` filter, `f` status and `o` sort run headless for `ls --view/--filter/--status/--sort` and `seen --all-done`, so the CLI lists what the board would |
| `src/tui/columns.rs`, `views.rs`, `status.rs` | Which columns a width shows; views (a grouping plus a sort); the `f` status filter |
| `src/tui/card.rs` | The turn card: the selected row's bar, headline, ask or body, tail, and its facts at the foot |
| `src/tui/meta.rs` | The card's facts: host, harness, turn, where, extension fields, title and source, each once under a label, in aligned columns |
| `src/tui/icons.rs` | The Nerd Font glyphs the facts take with `"icons": "nerd"`, pinned by name and code point; nothing else in `src/tui/` writes a private-use glyph, and a test checks it |
| `src/tui/full.rs`, `vt.rs` | The full view, read again every 2s; `vt.rs` keeps only a line's colours (SGR) and turns them into styles |
| `src/tui/answer.rs`, `screen.rs` | Answer mode: its keys and card, and what it reads off a pane (options, the input line, a draft) |
| `src/tui/clipboard.rs` | `yy` and `yp`: `pbcopy`, `wl-copy` or `xclip` |
| `src/tui/theme.rs` | The one style system: the roles text takes (`quiet`, `strong`, `accent`, `alert`, `note`, `header`, `selected`, `cut_mark` and the rest), and `face`, how each row state looks (its glyph or animation frames and their interval, the glyph it holds under reduced motion, colour and modifiers). Nothing else in `src/tui/` picks a colour or a glyph, and a test checks it. The board redraws at the shortest interval of an animated glyph on screen (the spinner's 160 ms), otherwise once a second |
| `src/tui/prefs.rs` | `tui.json` |

serve uses std threads and channels, with no async runtime. Collectors, extension runners, the socket listener and each connection run on their own thread, and only the reducer thread touches the board. A runner sends its results to the reducer and never waits on it, so a slow adapter cannot hold up rows.

## Files

| Path | What |
| --- | --- |
| `$XDG_STATE_HOME/rexboard/` (default `~/.local/state/rexboard/`) | The state dir, mode 0700 |
| `serve.lock` | The `flock` that keeps serve to one instance, plus `{pid, rex, children}`: serve's pid, the `rex` path and its observers' pids |
| `serve.sock`, beside `serve.lock` | The socket, mode 0600. It depends only on the state dir, not on `TMPDIR` or `XDG_RUNTIME_DIR`, so every caller finds the one serve. A path over 103 bytes fails with `SERVE` (set a shorter `XDG_STATE_HOME`). A client refuses a socket served by another uid |
| `restart.lock` | Held by the one command replacing an older serve, so commands that find it at once restart it only once |
| `state.json` | `{"v":1,"panes":{KEY:{state,kind,since,since_estimated,turn_started,last_turn,seen}},"jumps":[KEY,KEY]}`, where `jumps` holds the last two jumps, older first, for `jump -`. Written through a temp file and a rename at most once a second, and on stop. Entries for a host that has not sent its first snapshot this run are kept as loaded |
| `transitions.jsonl` | One line per state change: `{at, host, block_id, from, to, kind}`. Over 5 MB at serve start, it moves to `transitions.jsonl.1`, replacing the older one |
| `serve.log` | serve's output when a command started it. A client clears it first when it is over 1 MB |
| `tui.json` | The board's own settings on this machine, `{"detail_hidden": BOOL, "motion": "full" \| "reduced", "icons": "text" \| "nerd", "card_height": ROWS \| "N%"}`: whether `c` hid the turn card, whether the spinner holds still, whether the card's facts take Nerd Font glyphs, and the card's height. The board writes it on each `c`, keeping `motion`, `icons` and `card_height`; a missing or unreadable file means shown, full motion and words, and an old `detail` key is ignored |

On start, serve takes the lock, removes a stale socket, then stops observers a dead serve left behind. A recorded pid is stopped only if it is still its own process group, its parent is init or the recorded serve, and its command is the recorded `rex` running `do - --all --fail-fast`. Anything else is skipped and logged, because the pid may now belong to another program.

## Protocol

One JSON request per line on the socket; every reply line carries `"v": 1`.

| Request | Reply |
| --- | --- |
| `{"cmd":"rows"}` | `{"v":1,"rows":[ROW...],"hosts":{HOST:STATE},"host_errors":{HOST:WHY},"extensions":{...},"fields":[...]}`. `STATE` is `starting`, `up`, `down` or `stopped`; `host_errors` says why for a host that is down or stopped, until it is up again. `extensions` is each extension's last run, and `fields` the declared fields (`{id, name, title}`) in `enabled` order, sent before any run so the board can show them. Right after serve starts, the reply waits up to 5s for every host's first snapshot |
| `{"cmd":"subscribe"}` | The same `rows` line now and on every change (at most every 100ms), plus `{"v":1,"heartbeat":true}` every second. Each subscriber has room for 4 lines; a reader that falls behind misses lines rather than growing serve, and the next `rows` line is a full snapshot again |
| `{"cmd":"seen","key":"HOST/BLOCK_ID","jump":false}` | `{"v":1,"ok":true,"key":KEY}`. `jump: true` (from a jump) also makes it where `next` without `after` starts |
| `{"cmd":"next","after":KEY,"back":false}` | `{"v":1,"row":ROW}`: the next `needs_you` row after `after` (a key or a block id; else the last jump's pane; else the first), unseen rows first and all of them once every row is seen, wrapping; `back` goes the other way |
| `{"cmd":"back"}` | `{"v":1,"row":ROW}`: the pane the jump before the last one went to, for `jump -`; else `NOT_FOUND`, `no previous jump yet` or `previous pane is gone` |
| `{"cmd":"stop"}` | `{"v":1,"ok":true}`, then serve saves and exits |
| `{"cmd":"version"}` | `{"v":1,"rexboard":"0.9.0"}`, answered at once. Every client asks first and replaces a serve older than itself. A serve that answers `USAGE` predates the question, so it counts as older |

Errors are `{"v":1,"error":{"code":CODE,"message":...}}`, with `CODE` one of `USAGE`, `NOT_FOUND` and `NOBODY_NEEDS_YOU`. `ROW` is the object `ls --json` prints.
