# CLI

`rexboard --help`, and `--help` on each command, is the full command contract. This page covers output, fields and exit codes. Every command takes `--json`: JSON on stdout, and errors as JSON on stderr, `{"ok":false,"error":{"code":...,"message":...,"next_steps":[...]}}`. Exit 1 is a failure; exit 2 is bad usage, or every host unreachable (`HOSTS_UNREACHABLE`). Any command can also fail with `IO` (exit 1) when a file it needs cannot be read or written. `rexboard --version` prints the version.

## Finding Rex

rexboard runs the `rex` CLI. It uses the first of:

1. `$REXBOARD_REX`
2. `$REX_BIN_DIR/rex`
3. `rex` on `PATH`
4. the macOS app bundle: `~/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex`

With none of them, a command fails with `REX_NOT_FOUND`, next steps `set REXBOARD_REX to the path of the rex CLI` and `install Rex`.

## `rexboard ls`

```bash
rexboard ls                 # agent panes on every host, most urgent first
rexboard ls --all           # plain shells too
rexboard ls --host mini     # one host: local, or a label from `rex hosts`
rexboard ls --json          # an array of panes
rexboard ls --direct        # one snapshot here, without serve
rexboard ls --view host     # grouped and ordered as a board view (`rexboard views`)
rexboard ls --filter host:mini --filter tests   # as the board's `/`; each one narrows
rexboard ls --status q      # one status, by name or first letter, as the board's `f`
rexboard ls --sort age:desc # within each group, as the board's `o`
```

`ls` asks [`rexboard serve`](#rexboard-serve) for its rows, starting it when nothing answers, so the rows carry what serve remembers: `since`, `seen` and the turn fields. `--direct` skips serve: one embedded Lua script runs through `rex do -` and reads this host plus every `rex hosts` entry in one call. A call that takes more than 10 seconds fails with `REX`. `ls --direct` sees one snapshot, so every `done` is unseen and `last_turn` is `null`.

A pane is listed when it has a status record, a known harness, or a fallback status from an [extension](extensions.md); `--all` adds the rest. The rows and their order are the same as `watch --json`: by `group`, then, inside `needs_you`, `blocked` before `error` before unseen `done`, then the longest in its state first.

The text table has these columns:

- `GROUP` and `STATE`.
- `AGE`: time in the current state, as `34s`, `12m`, `5h43m` or `3d4h`. `≥` means estimated, and `-` means the pane has no status record, so its time cannot be known.
- `HARNESS`: `claude`, `codex` and so on, with a `?` when the pane has no status record (the harness then comes from its process or title); `-` when unknown.
- `HOST`.
- `WHERE`: `session › tab › pane`, each label cut to 24 columns with `…` in the middle, so a long tab label keeps both ends. A pane labelled `shell`, or named after its foreground process, tells you nothing, so it shows as `session › tab`.
- `WHAT`: the record's message, else the pane title without its leading state glyph.

In a terminal each line fits its width: `WHAT` is cut first, then `WHERE` (a long tab first, down to `…` alone below 6 columns, then the session down to an 8-column stem such as `api-…way`, then the last label down to 6 columns, and only then a short tab and the stems). The ` › ` separators always stay. Rows that would still read the same end in `#N`, the pane's place among its tab's agent panes, oldest first. Piped output keeps every column whole. Cells are printed without escape sequences or control characters, since titles and messages come from the programs in each pane. Wide characters such as CJK and emoji count as two columns. `--json` keeps every label and title whole, and the full harness name (`claude-code`).

`--view`, `--filter`, `--status` and `--sort` run the board's own code, so `ls` lists what the board would. `--view NAME` groups and orders as that view does: `status`, `table`, `harness`, `host`, or one per extension field (`rexboard views` lists them). `--filter TEXT` keeps the rows the board's `/` would: words match any label, the message or the harness as one phrase, and `key:value` matches one key (`host`, `state`, `harness`, `session`, `window`, `block`, or a field's name); every term must match, ignoring case. A `key:` that names no key is plain text, as on the board, so `failed: 2` or a URL searches the message; stderr gets a `note:` naming the keys there are. Each `--filter` narrows further. `--status` takes `working`, `done`, `question`, `permission`, `error`, `unreported` or `idle`, or its first letter. `--sort COLUMN` sorts within each group by a column the board can sort by (`state`, `age`, `turn`, `harness`, `host`, `where`, `session`, `window`, `block`, `msg`) or a field, in the direction its first `o` press takes; `:asc` or `:desc` picks one. An unknown view, status, column or direction fails with `USAGE` and lists the ones there are. None of them work with `--direct`. Without them the order is serve's, as above.

### JSON fields

`--json` prints one array. Each pane has every field below; a missing value is `null`.

| Field | Type | Meaning |
| --- | --- | --- |
| `key` | string | `HOST/BLOCK_ID`, what `rexboard seen` and `rexboard jump` take |
| `host` | string | `local`, or the label from `rex hosts` |
| `session_id` | string | Rex session id |
| `session` | string | Session label |
| `window_id` | string | Rex window id |
| `tab` | string | Window (tab) label |
| `block_id` | string | Rex block id of the pane |
| `pane` | string | Block (pane) label |
| `title` | string | Terminal title, `""` when none |
| `process` | string or null | The pane's foreground process, as in `zsh`, `tmux`, or a version such as `2.1.310` (Claude Code runs under its version number) |
| `tmux` | boolean | `true` when the pane shows a tmux client: `tmux` in front, or the `/bin/sh -c ... tmux attach` wrapper OpenRig runs around one (a shell started as `/bin/sh`). It is true with or without a status |
| `pid` | integer or null | That process's pid on its host |
| `harness` | string or null | `effective_app` (else `app`) of the record, else the foreground process (`codex`, `pi`; `claude`, or a version string like `2.1.310`, is `claude-code`), else `claude-code` for a title that starts with a Claude state glyph, else `codex` for a tmux pane titled `DIR | MODEL` (exactly one ` | `, a `DIR` with no spaces, a `MODEL` of letters, digits, `.` and `-` with at least one letter and one digit) |
| `state` | string | `blocked`, `error`, `done`, `working` or `idle` from the record; with no record, from the title glyph (`✳ ` is `idle`, a braille spinner or `◐`/`◑` is `working`), else `unknown` |
| `kind` | string or null | The record's `kind`, such as `permission` |
| `msg` | string or null | The record's message, ` ⏎ ` between lines; from `rexboard hook`, an empty line between paragraphs and each line's leading spaces kept |
| `updated_at` | string or null | RFC 3339 time Rex received the record. It changes on every report, not only on a state change |
| `source` | string | `native`: the agent in the pane reports its own status. `bridge`: the record came through tmux (tmux wrote it, or is the pane's foreground process, or the foreground is a shell started as `/bin/sh`, OpenRig's `sh -c ... tmux attach` wrapper). `inferred`: no record, state read from the title. An extension's name: no record, and that extension gave the row a fallback status. See [Agent status](agent-status.md) |
| `children` | object | Counts by state of the pane's other records, such as Claude subagents: `{"working": 1}` |
| `since` | string | RFC 3339 UTC time the pane entered its current `state` and `kind`. A report in the same state keeps it |
| `since_estimated` | boolean | `true` when rexboard did not see the change: a row first seen in a snapshot that is `working` (its `updated_at` is the last report, not the start) or has no record, or an `updated_at` that is not RFC 3339. That last case also prints `warning: HOST: updated_at "VALUE" is not RFC 3339 (Rex VERSION); time in state is estimated` on stderr, once per host. A row with no record still has a `since` in `--json`; the text table and the board show its age as `-` |
| `turn_started` | string or null | When the pane last entered `working` |
| `last_turn` | integer or null | Seconds from entering `working` to the next `done` or `blocked`. Short when the turn was already running when rexboard started |
| `seen` | boolean | `false` after the pane enters `done`, `blocked` or `error`; `true` once it enters `working` again. A `done`, `blocked` or `error` first seen in a snapshot starts unseen. `rexboard seen KEY`, a jump, or `m` on the board marks it seen |
| `stale` | boolean | `true` while the host's observer is down; the next snapshot clears it |
| `suspect` | boolean | `true` when the record says `working`, its last report is more than 120 seconds old, and the pane disagrees: its title starts with Claude's idle `✳ ` (not checked when `tmux` or that wrapper is in front, since the title is then tmux's and keeps the idle glyph while the agent works), or a shell (`zsh`, `bash`, `fish`, `sh`) is back in front. A hook that never reported the end of a turn leaves this behind. `state` keeps the record's value; the board and the `ls` table show `idle?` |
| `suspect_reason` | string or null | Why, as in `reported working at 14:02; the title shows idle`. A report a day old or more gives its date: `reported working at Oct 24 14:02; zsh is back in front` |
| `group` | string | `needs_you` (`blocked`, `error`, unseen `done`), `working`, `quiet` (`idle`, seen `done`), or `unreported` (`source` is `inferred`, `state` is `unknown`, `stale`, or `suspect`) |
| `ext` | object | Fields from [extensions](extensions.md), by id: `{"session-prefix.project": "api"}` |
| `usage` | object or null | What the agent's last turn used, for a pane on serve's own host whose harness runs [`rexboard usage`](#rexboard-usage); `null` otherwise. It stays while the pane lives, at any age. Only the figures the harness reported: `model` (string), `ctx_pct` (0-100), `ctx_window` and `tok` (tokens), `cost_usd` (number), `quota_5h` (0-100): `{"model": "Opus 5.5", "ctx_pct": 82, "tok": 820000}`. `ls --direct` has no serve, so it is always `null` there |

The pane's main record is the one with no `id`; if every record has one, the most urgent wins.

Fields are a contract. New fields may appear. Renaming or removing one is a breaking change. Key order is not part of it; read fields by name.

### Exit codes and errors

| Exit | When |
| --- | --- |
| 0 | Listed. A host that cannot be reached prints `warning: HOST is unreachable: ...` on stderr (through serve: one line per host that is `starting`, `down` or `stopped`), and the rest are listed. When a host is `down` or `stopped`, the warning and the `HOSTS_UNREACHABLE` error end with `; see STATE_DIR/serve.log` |
| 1 | `REX_NOT_FOUND`: no `rex` CLI. `REX`: `rex do` failed, timed out, or returned a snapshot rexboard does not understand; the message names the Rex version. `SERVE`: serve did not start or answer; the message names `serve.log`. `CONFIG`, with `--direct`: config.toml cannot be read or has an unknown key |
| 2 | `HOSTS_UNREACHABLE`: every host failed; next steps `rex hosts` and `rexboard ls`. `USAGE`: a bad flag, `--host` names no host, or `--view`, `--status` or `--sort` names nothing |

## `rexboard watch`

```bash
rexboard watch --json               # the agent rows from every host, again after each change
rexboard watch --json --direct      # the same rows from this command's own observers
rexboard watch --raw                # every event from every host, as it happens
rexboard watch --raw --host mini    # one host: local, or a label from `rex hosts`
rexboard watch --raw --for 60       # stop after 60 seconds
```

`watch` needs `--json` or `--raw`; with neither, it fails with `USAGE`. With both, `--raw` wins and errors print as JSON.

`watch --json` subscribes to [`rexboard serve`](#rexboard-serve), starting it when nothing answers, and exits 1 with `SERVE` if serve stops. `--direct`, and `--raw` always, run this command's own observers instead: rexboard reads the host list once from `rex hosts --json`, adds this host as `local`, and keeps one embedded Lua observer running per host through `rex [-S HOST] do - --all --fail-fast`. A host added after a direct `watch` starts needs a restart. Without `--for`, `watch` runs until Ctrl-C, SIGTERM, or stdout closes.

### Rows (`--json`)

Each line is one JSON array: the agent rows from every host, in the same shape and order as `ls --json`. A new line follows each change to any row, at most one every 100ms, so a burst of reports prints once. A host's `down` marks its rows `stale`; its next snapshot replaces them, and `since`, `seen` and the turn fields survive for panes whose `state` and `kind` did not change. A host that sends a line or record rexboard cannot read restarts its observer, as [Down and restart](#down-and-restart) describes; its rows stay `stale` until the new snapshot, and the other hosts keep printing. Pending rows print once more when `watch` stops.

### Lines (`--raw`)

Each line is one JSON object. Every line has these three fields:

```json
{"block_id":"block:web-seat-b","host":"mini","session_id":"session:web","title":"Fix the parser","type":"title","v":1}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `host` | string | `local`, or the label from `rex hosts` |
| `v` | number | Line contract version, `1`. A new major version is a breaking change |
| `type` | string | One of the types below |

A missing value is `null`. `ids` is `[]` when empty.

| `type` | Other fields | When |
| --- | --- | --- |
| `snapshot` | `sessions`, `rex_version` | First line from each observer, and again after every restart. `sessions` is the full session, window and block tree in Rex's shape |
| `status` | `session_id`, `block_id`, `record_id`, `record` | A pane reported a status. `record` is Rex's record (`state`, `kind`, `msg`, `app`, `effective_app`, `owner`, `updated_at`). `record_id` is `""` or `null` for the pane's main record, else the sub-record's id, such as a Claude subagent |
| `status_removed` | `session_id`, `block_id`, `ids`, `reason` | Status records were cleared |
| `title` | `session_id`, `block_id`, `title` | The terminal title changed |
| `process` | `session_id`, `block_id`, `name`, `argv0`, `cwd`, `pid` | The foreground process changed |
| `child_exited` | `session_id`, `block_id`, `exit_code` | The pane's process exited |
| `block_created` | `session_id`, `window_id`, `block` | A terminal pane opened. `block` has the same shape as a block in `snapshot` |
| `block_closed` | `session_id`, `block_id` | A pane closed |
| `window_created` | `session_id`, `window_id`, `label` | A tab opened |
| `window_closed` | `session_id`, `window_id` | A tab closed |
| `label` | `kind`, `session_id`, `id`, `label` | A session or tab was renamed. `kind` is `session` or `window`; `id` is that session or window id |
| `active_window` | `session_id`, `window_id` | The focused tab changed. `window_id` is `null` when the session's last tab closed |
| `session_created` | `session` | A session started. `session` has the same shape as one session in `snapshot`: `session_id`, `label`, and `windows` with their blocks. A pane's title and process can still be empty here; `title` and `process` lines follow |
| `session_destroyed` | `session_id` | A session ended |
| `tick` | none | Every 30 seconds per host, so a quiet host still shows it is alive |
| `down` | `reason`, `retry_in_s` | rexboard lost the observer. See below |

A line of a type not listed here passes through as Rex sent it, with `host` and `v` added. It is safe to ignore.

Types may be added. Renaming or removing a type or field is a breaking change.

### Down and restart

When an observer exits, its output can't be read, or it prints nothing for 90 seconds (three missed ticks), rexboard prints a `down` line, waits `retry_in_s` seconds, and starts a new observer, which begins with a fresh `snapshot`. A Lua handler that fails also ends the observer (`--fail-fast`), and the last line it printed on stderr is added to `reason`:

```json
{"host":"mini","reason":"observer exited (exit status: 1): observer.lua:12: attempt to index a nil value","retry_in_s":1,"type":"down","v":1}
```

The wait starts at 1 second and doubles up to 30. It goes back to 1 only after a run that lasted at least 30 seconds, so an observer that fails on every event backs off instead of restarting every second. A host that keeps going down never stops `watch`.

A line rexboard cannot read restarts that host's observer the same way: a line that is not JSON, a known type with a bad field, or (with `--json` and in `serve`) a record the board rejects, such as a status without `state`. stderr gets `warning: HOST: ERROR; restarting its observer: LINE`, with the line cut at 500 characters. Until the new snapshot the host's rows are `stale`, about 1 second after a one-off line. If every snapshot carries the same line, the host stays `stale`, retries with the same doubling wait, and logs the warning each time. Only a different `v` (`CONTRACT_MISMATCH`) stops a host for good: `warning: HOST stopped and will not restart: ...`, and the other hosts keep streaming.

### Exit codes and errors

| Exit | When |
| --- | --- |
| 0 | `--for` elapsed, Ctrl-C or SIGTERM, or stdout closed |
| 1 | `REX_NOT_FOUND`: no `rex` CLI. `REX`: `rex hosts` failed. `SERVE`: `watch --json` lost serve. `CONFIG`, with `--raw` or `--direct`: config.toml cannot be read or has an unknown key. `CONTRACT_MISMATCH`: every host stopped and the last one stopped on a version mismatch; next steps `rexboard --version` and `rexboard upgrade` |
| 2 | `USAGE`: neither `--json` nor `--raw`, a bad flag, or `--host` names no host |

## `rexboard board`

```bash
rexboard          # in a terminal, with no command
rexboard board
```

The board: the agent panes `ls` lists, grouped as `NEEDS YOU`, `WORKING`, `QUIET` and `UNREPORTED`, with a ticking time in state. It is a client of [`rexboard serve`](#rexboard-serve) and starts it when nothing answers. `board` without a terminal fails with `USAGE`. [TUI](tui.md) has the layout and keys.

## `rexboard serve`

```bash
rexboard serve                # start it in the background, or say it is already running
rexboard serve --foreground   # run it in this terminal
rexboard serve --stop         # stop it and its observers
```

`serve` owns one observer per host and the board, and answers `ls`, `watch --json`, `seen`, `show`, `answer`, `close`, `views`, `hosts`, `jump`, `next`, `pick`, the board and `ext test` over a unix socket. Any of those starts it when nothing answers, as `rexboard serve --foreground` in its own session with its output in `serve.log`, and waits up to 2 seconds for it. Only one runs at a time: a second `serve --foreground` fails with `SERVE_RUNNING` (exit 1). It rescans `rex hosts` every 5 minutes.

The socket, `serve.sock`, sits beside `serve.lock` in the state dir, so every caller finds the running serve whatever its `TMPDIR` or `XDG_RUNTIME_DIR`. If that path is longer than 103 bytes, commands fail with `SERVE`: set a shorter `XDG_STATE_HOME` everywhere rexboard runs (it also holds `state.json`). serve keeps the environment of the command that started it, so after changing `REXBOARD_REX` or `REX_BIN_DIR`, run `rexboard serve --stop`.

serve keeps `since`, `seen` and the turn fields in `state.json`, and puts them back after a restart for every pane whose `state` and `kind` did not change. It appends each state change to `transitions.jsonl`. [System architecture](system-architecture.md) has the protocol and the files.

### Upgrades take effect by themselves

Every command that talks to serve (`ls`, `watch`, `seen`, `next`, the board, and `serve` itself) first asks serve its version. When serve is older, the command stops it, starts its own, and says so once: `restarted serve 0.8.0 -> 0.9.0`. A newer serve, or one whose version it cannot read, is kept, so two installed versions never take turns restarting it.

A serve that holds `serve.lock` but does not answer, such as a stuck one, is replaced the same way when it is a `rexboard` process: `restarted serve (pid N did not answer) -> 0.9.0`. Only a holder that was there before the command's 2-second wait, and still is after it, is stopped, so a serve that is still starting is never touched. Commands that find the same old serve at once take turns, and only the first restarts it; a command waits at most 30 seconds for its turn, then fails with `SERVE`. The line goes to stderr and to `serve.log`; from the board it goes only to `serve.log`, so the screen stays whole.

A lock holder that is not rexboard is left alone. The command fails with `SERVE`, saying whether the lock names no pid, a pid that is not running, or one that is not rexboard, with `lsof` and `kill` as the next steps. `serve --stop` refuses it the same way.

### `serve --stop`

It exits 0 whether or not serve was running. A serve that holds the lock but does not answer gets SIGTERM, but only when the pid in `serve.lock` is a `rexboard` process: by its executable's path, or by its process name when the path cannot be read, as on macOS once an upgrade has deleted the binary it runs. A serve that ignores SIGTERM is named with `kill -9 N` as the next step. `--stop` waits up to 5 seconds for serve's process to exit, then fails with `SERVE`.

`serve --json` prints `{"ok":true,"pid":N,"already_running":BOOL,"socket":PATH}` (`pid` is `null` when unknown), and `serve --stop --json` prints `{"ok":true,"running":false}`.

## `rexboard seen`

```bash
rexboard seen mini/block:web-seat-b
rexboard seen --all-done                    # every unseen done row, as the board's :seen
rexboard seen --all-done --host mini --filter api
```

Marks a pane seen, by the `key` from `ls --json`: an unseen `done` leaves `needs_you`. serve keeps it across restarts until the pane changes state again. It prints `seen KEY`; with `--json`, `{"ok":true,"key":KEY}`. An unknown key fails with `NOT_FOUND` (exit 1), and a key without `/` with `USAGE` (exit 2).

`--all-done` marks every unseen `done` agent row seen, as the board's `:seen` does; `--host` and `--filter` (the same as `ls --filter`, and repeatable) narrow which. It prints `2 done rows marked seen`, or `no unseen done rows`; with `--json`, the keys it marked:

```json
{"keys":["mini/block:web-seat-b","local/block:api-claude"],"ok":true}
```

`--host` and `--filter` without `--all-done`, or `--all-done` with a key, fail with `USAGE`.

## `rexboard show`

```bash
rexboard show mini/block:web-seat-b            # what the board's card says
rexboard show mini/block:web-seat-b --tail 20  # and the pane's last 20 lines
```

Everything the board's card says about a pane, by the `key` from `ls --json`, plain shells included, in the card's order: the bar (`blocked · permission · seat-b · waiting 2m`), the message in full, the tail with `│` before each line, then where, host, harness, last turn, model and ctx (when its harness reports them; `ctx 88% · 176k of 200k`), cost and 5h (when `tui.json` asks for those columns), extension fields, title, source and key. `--tail N` reads the pane's last N lines (at most 200) the way the card does: the end of its scrollback through `tail.lua`, plain text, within 3 seconds. With `--json`, the pane as `ls --json` has it, plus `where` (`session › window › block`); with `--tail`, `tail`, an array of lines; and what the pane shows now, read the way [`answer`](#rexboard-answer) reads it: `dialog`, a short id of the dialog or input line on screen (`null` with neither), and `options`, the dialog's options, each with whether it grants something for later. A dialog's id is its lines, so the same dialog keeps it through a second report; an input line's also takes the row's `updated_at`, since the same empty prompt comes back every turn. When the pane cannot be read (its host is down, or it does not answer within 3 seconds), the row still prints, with `dialog` `null`, `options` `[]` and the reason in `screen_error`:

```json
{"key":"mini/block:web-seat-b","host":"mini","session":"web","tab":"main","pane":"seat-b","state":"blocked","kind":"permission","msg":"Bash wants to run: cargo test","source":"native","seen":false,"group":"needs_you","last_turn":372,"ext":{},"where":"web › main › seat-b","tail":[" Do you want to proceed?"," ❯ 1. Yes","   2. Yes, and don't ask again for cargo test"],"dialog":"3f9a1c07be42","options":[{"digit":1,"label":"Yes","lasting":false},{"digit":2,"label":"Yes, and don't ask again for cargo test","lasting":true}]}
```

(The sample leaves out some of the fields `ls --json` lists.) An unknown key fails with `NOT_FOUND` (exit 1), a key without `/` with `USAGE` (exit 2), and a `--tail` that cannot be read with `REX` (exit 1).

## `rexboard answer`

```bash
rexboard show mini/block:web-seat-b --json                                   # read it: dialog 3f9a1c07be42
rexboard answer mini/block:web-seat-b --dialog 3f9a1c07be42 --pick 1         # option 1 of that dialog
rexboard answer mini/block:web-seat-b --dialog 3f9a1c07be42 --pick 2 --confirm   # an option that lasts
rexboard answer mini/block:web-seat-b --dialog 3f9a1c07be42 --text "run the tests"  # into the input line
```

What the board's [answer mode](tui.md#answer-mode) does, for a script or an agent, with its checks. Pass what you read: `--dialog` is the `dialog` from `show KEY --json`, and the answer goes only to that dialog or input line. The board's answer mode picks from the screen it drew; a script reads first and acts later, and a pane can bring up another dialog with the same options in between (two Bash commands, two edits), or finish a turn and show the same empty prompt again, so `answer` reads the pane again and refuses when its `dialog` is not the one passed. Then one `rex do` on the pane's host checks again that the agent's state and kind, the agent in front, and the whole dialog or input line are as read, and sends. `--pick N` sends option N's digit, one key press and no Enter. An option that grants something for later (`always`, `don't ask again`, `auto mode`, `this session`, `accept edits`), which the board takes only on a second press, needs `--confirm`. `--text` pastes the line, then presses Enter, and only into an empty input line: never into a dialog, or a line that already holds text. `--expect STATE[:KIND]` sends only while the row is still in that state, and kind, as `ls --json` has them (`blocked`, `blocked:permission`). Esc and Ctrl+C are never sent. A reply marks the row seen, as on the board. With `--json`:

```json
{"ok":true,"key":"mini/block:web-seat-b","sent":{"pick":1,"label":"Yes"}}
```

or `"sent":{"text":"run the tests"}`. Without it, `sent 1 (Yes) to web › main › seat-b on mini`.

A check that fails sends nothing. It fails with `REFUSED` (exit 1), says why, and names the next step:

| Refusal | Next step |
| --- | --- |
| `the dialog changed since you read it` (or `the input line`), no dialog or input line on the screen now, an input line holding text, the pane changed between the read and the send (`the screen changed since it was read`), or a send that timed out, which may or may not have reached the pane | `rexboard show KEY --json` |
| no such option, or text into a dialog: the message lists the options, and `--json` has them as `error.options` | `rexboard answer KEY --pick 1 --dialog ID` |
| a lasting option without `--confirm` | `rexboard answer KEY --pick N --confirm --dialog ID` |
| a row not in `--expect`'s state | `rexboard show KEY` |
| a pane with no status record | `rexboard jump KEY` |
| your own pane: the one in `REX_BLOCK`, outside tmux | none |

An unknown key fails with `NOT_FOUND` (exit 1). Bad usage fails with `USAGE` (exit 2): no `--dialog`, no `--pick` or `--text`, both, a digit outside 1-9, or an empty `--text`. A screen that cannot be read fails with `REX` (exit 1).

## `rexboard close`

```bash
rexboard close mini/block:web-seat-b         # asks first, on a terminal
rexboard close mini/block:web-seat-b --yes   # closes without asking
```

Closes a pane with `rex [-S HOST] block close`, as the board's `dd` does. On a terminal it asks first, the way `dd` does, and only `y` closes: `close web › main › seat-b on mini, ending claude (blocked · permission)? [y/N]`, with `(tmux keeps running)` for a tmux client; anything else says `nothing closed` and exits 1 with `CANCELLED`. Without a terminal it never asks: `--yes` closes, and without it the command fails with `USAGE` (exit 2) and the question, so nothing is closed by accident. With `--json`:

```json
{"ok":true,"key":"mini/block:web-seat-b","closed":true}
```

The pane that runs the board, any pane running `rexboard`, and your own pane are never closed (`REFUSED`, exit 1). Outside tmux your own pane is the one in `REX_BLOCK`. Inside tmux, `REX_BLOCK` is the Rex pane that started the tmux server, not yours, so a pane on this host that runs tmux may be the one you are in, and `close` refuses it from inside tmux; close it from the board or from outside tmux. Rex refusing the close fails with `REX` and Rex's words (`close failed: error: block not found`); an unknown key with `NOT_FOUND`.

## `rexboard usage`

```bash
rexboard usage claude -- my-status-line   # as Claude Code's statusLine command
rexboard usage codex                      # from a Codex Stop hook
```

Keeps what an agent's last turn used, for the board's `MODEL` and `CTX` (and `TOK`, `COST` and `5H` when `tui.json` asks for them; see [the board](tui.md#layout)) and the `usage` field of `ls --json` and `show --json`. The harness runs it from its own per-turn surface with its JSON on stdin, so rexboard never reads a transcript:

| Harness | Runs it from | Reads |
| --- | --- | --- |
| `claude` | `statusLine` | `model`, `context_window` (`used_percentage`, `context_window_size`, `total_input_tokens`), `cost.total_cost_usd`, `rate_limits.five_hour` |
| `grok` | its status line command | the same fields, as far as grok sends them (`model`, `context_window.used_percentage`) |
| `codex` | the `Stop` hook | `model`, then the last `token_count` event in the last 64 KiB of `transcript_path`: CTX as Codex's footer counts it, `(total_tokens - 12000) / (model_context_window - 12000)`, and TOK as `last_token_usage.input_tokens` |
| `pi` | its extension, at a turn's end | `{"model", "context": ctx.getContextUsage(), "usage", "cost"}`, where `usage` is the last assistant message's: TOK is its `input + cacheRead + cacheWrite`, the prompt side, since the context's `tokens` counts the reply too |

It keeps one small file per pane in `usage/` in the state dir, named for the pane: `REX_BLOCK` in a plain Rex pane, or inside tmux the tmux socket, server pid and `TMUX_PANE`, where `REX_BLOCK` names the pane that started the tmux server. Serve reads a file again only when it changes, and keeps a pane's figures while the pane lives, at any age: a harness writes only when it works. A file that no pane on this host names (a Rex pane, or any pane on a tmux server a local tmux client talks to) is removed once it is 10 minutes old (Rex never reuses a block id, and a restarted tmux server has a new pid). Figures a harness does not report stay blank, never zero, and so do a token count of 0 and a cost of 0 (before the first request, or a proxy or a subscription).

It prints nothing itself. `-- CMD …` runs your own status line command with the same JSON and passes its output and exit code through, so the status line looks as it did. A payload it cannot read, or a pane it cannot name, keeps nothing and still runs `CMD`: it never breaks the status line it runs in. Nothing else makes it fail either: an unknown harness, or an argument this build does not know, is said on stderr and in `REXBOARD_HOOK_LOG`, and it exits 0, or with `CMD`'s code when there is one, which it still runs.

## `rexboard hook`

```bash
rexboard hook claude Stop    # a Claude Code hook, with the hook's JSON on stdin
```

Tells Rex an agent's status from one of its hooks. The hooks `rexboard hooks install` writes run it. It reads the hook's JSON on stdin, turns the event into a status, and writes the OSC 7501 sequence where Rex sees it:

| Where the agent runs | What it writes |
| --- | --- |
| Inside tmux: `TMUX_PANE`, or under a tmux `TERM`, the one tmux pane on the agent's tty | The sequence in tmux passthrough to that pane, which forwards it to the Rex pane running `tmux attach`. It also keeps it on the tmux pane (`@rex_status`) for [`hooks replay`](#rexboard-hooks) |
| Straight in a Rex pane (`REX_BLOCK`) | pi: the sequence, to pi's terminal. Claude Code reports its own state there, so only a `turn` record with the final message ([Agent status](agent-status.md)). Codex reports its own: nothing |
| Neither | Nothing |

| Event | Status |
| --- | --- |
| `SessionStart` | `idle` |
| `UserPromptSubmit` | `working`, `Working` |
| `PreToolUse` | `working`, the tool's description, else its name. For `AskUserQuestion`: `blocked` `question`, with the first question and its numbered options |
| `PostToolUse` of `AskUserQuestion` | `working`, `Working` |
| `PermissionRequest` | `blocked` `permission`, `Allow TOOL?` (Codex: its description) |
| `Notification` of type `permission_prompt` | `blocked` `permission`, the notification's message |
| `Stop` | `done`, the final message, else `Done` |
| `StopFailure` | `error`, the message so far, else the error |
| `SessionEnd` | clears the status |

Any other event changes nothing. A message goes on one line, ` ⏎ ` for each line break, cut to Rex's 2048 bytes and to 4096 for the whole sequence. A run of blank lines leaves one empty line (` ⏎  ⏎ `), so paragraphs survive, with none at the start or end. A line keeps its leading spaces, for code, with a leading tab as four; trailing spaces go, and any other control character is a space. So since 0.16 a `msg` in `ls --json`, `watch` and `show` can carry empty lines and indentation; its first line with words is the same as before. `EVENT` names the event for JSON that has no `hook_event_name`. On Codex's `Stop` it also keeps the turn's usage, as [`rexboard usage codex`](#rexboard-usage) does.

It prints nothing and exits 0 whatever happens, an unknown harness or an argument this build does not know included, since exit 2 is the code that blocks a Claude Code agent. Such an error is said on stderr and noted in `REXBOARD_HOOK_LOG`. `REXBOARD_HOOK_LOG=FILE` appends one line per call: the event, which of `TMUX`, `TMUX_PANE` and `REX_BLOCK` were set, the pane found, and what it did. It never logs the hook's text.

## `rexboard hooks`

```bash
rexboard hooks status                  # what each harness has, and what an install would change
rexboard hooks install --dry-run       # the same changes, written nowhere
rexboard hooks install                 # every harness this host has
rexboard hooks install claude codex    # only these
rexboard hooks uninstall --dry-run     # what an uninstall would take out
rexboard hooks uninstall               # exactly what install put in, after a backup
rexboard hooks replay SESSION          # from tmux, when a client attaches
```

`install` puts `rexboard hook` and `rexboard usage` into each harness's own settings:

| Harness | File | What it adds |
| --- | --- | --- |
| `claude` | `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` | `/bin/sh -c 'REXBOARD hook claude EVENT \|\| true'` for the nine events above (`PostToolUse` for `AskUserQuestion` only), each with a 5 s timeout, so a hook exits 0 even with rexboard gone or too old to know `hook`. The `statusLine` runs through `rexboard usage claude`: yours keeps running and printing, through rexboard while it answers `usage --help` and on its own when it does not. With no status line, `rexboard usage claude` |
| `codex` | `$CODEX_HOME/hooks.json`, else `~/.codex/hooks.json` | `/bin/sh -c 'REXBOARD hook codex EVENT \|\| true'` for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `Stop` and `SessionEnd`; `Stop` keeps the usage too |
| `pi` | `rexboard.js` in `$PI_CODING_AGENT_DIR/extensions`, else `~/.pi/agent/extensions` | An extension that runs `rexboard hook pi` on pi's events and `rexboard usage pi` at each turn's end |
| `grok` | | Nothing: grok reports to a Rex pane itself, and reads a status line only from `~/.grok/config.toml`, which rexboard does not edit. For its usage, add the lines below there by hand |

A settings file is merged, never replaced. Hooks that already report to Rex (rexboard's, or the `rex-agent-status` bridge it replaces) give way to rexboard's, where the first of them was; every other key and hook stays, in its order. Before it writes, the old file is kept beside it as `FILE.bak-<UTC>`, with the file's own permissions, and the write is a rename. A second install changes nothing. `REXBOARD` is rexboard by full path, since an agent may run its hooks with a shorter `PATH` than your shell's: the `rexboard` on `PATH` as found (`/opt/homebrew/bin/rexboard`, which stays put across upgrades) when it is this binary, else this binary's own path. Run `install` again after moving rexboard.

What `install` leaves alone:
- a harness whose settings directory is missing (`absent`);
- a settings file or pi's `rexboard.js` that is a symlink, or sits in a directory that is one, as a dotfiles checkout makes them (`symlink`): the note names the link, and you change it at its source. `status` and `install` read the file through the link without writing and say what it lacks (`~/.claude is a link into a checkout; missing: Stop, SessionEnd; change it there`), or that it has rexboard's hooks. Every directory from your home directory down counts (`~/.claude`, or `~/.pi` when stow folds it), or from the harness's own directory when its variable puts it elsewhere;
- a file that is not valid JSON, or whose `hooks` is not an object: `CONFIG` (exit 1), and nothing is written.

A directory with no settings file yet gets one. With pi's old `rex-status.js` extension still there, the line says so, as pi would report twice.

For grok's usage, in `~/.grok/config.toml` (end the command with ` -- YOUR-SCRIPT` to keep a status line of your own):

```toml
[ui.status_line]
type = "command"
command = "rexboard usage grok"
```

Inside tmux a status reaches Rex only through tmux passthrough, so tmux needs `set -g allow-passthrough on` in `tmux.conf`. `status` says when the default tmux server has it off.

`uninstall` takes out exactly what `install` put in, with the same rules (a backup first, a linked file or directory left alone, `--dry-run` and `--json` as for `install`):
- the hooks install writes, `REXBOARD hook HARNESS EVENT` alone or with `|| true`, inside `sh -c` or not. A hook of yours that also runs something else stays, as do your other hooks and keys, also ones added after install;
- Claude's `statusLine`: one install chained goes back to your own command, with its other keys as they are, and one install added goes. A status line install did not write is left alone;
- pi's `rexboard.js`, only while it is the file rexboard wrote. One you changed is `not ours` and stays;
- a file install made (no backup holds what it was) that holds nothing else now, such as Codex's `hooks.json` on a fresh machine, is removed after a backup. A file of yours is never removed.

When the newest backup that holds nothing of rexboard's is the file install turned into this one, uninstall writes that backup back, so the file comes back byte for byte, its layout and your empty events included. A `rex-agent-status` hook that install replaced does not come back: it exits 2, which blocks the agent, once its script is gone. A file that held one is stripped instead. When the file changed since install, there is no such backup, and uninstall takes out rexboard's hooks and any group, event or `hooks` key they alone filled. A second uninstall changes nothing. Codex's `config.toml` is never touched by either. Run `uninstall` before removing rexboard: until then each hook fails on every event. Moving `FILE.bak-<UTC>` back over the file also undoes an install. A second backup in the same second takes `-2`, so none is ever written over.

Each harness gets one line, `HARNESS STATE FILE`, then each change: `EVENT: add …` (`, keeping your 1 hook` when the event has others), `EVENT: replace OLD with …`, or `EVENT: remove …`. `status` ends with `rexboard hooks install writes these` when there is anything to write, a dry run with `dry run: nothing written; rexboard hooks install writes these` (or `uninstall`), and `status` adds a `tmux` line when a tmux server runs:

| State | Meaning |
| --- | --- |
| `installed` | rexboard's hooks are there and current |
| `not installed` | uninstall: nothing of rexboard's is there |
| `changes` | an install or uninstall would change what follows |
| `written` | this run wrote them; the old file's name follows |
| `removed` | uninstall removed a file install made; the backup's name follows |
| `not ours` | uninstall: pi's `rexboard.js` is not the one rexboard wrote |
| `absent`, `symlink`, `not managed` | left alone, as above |

With `--json` (`tmux` from `status` only; `null` with no tmux server):

```json
{"harnesses":[{"harness":"claude","file":"~/.claude/settings.json","state":"changes","changes":["PreToolUse: add `rexboard hook claude PreToolUse`, keeping your 1 hook","Stop: replace `rex-agent-status` with `rexboard hook claude Stop`"]},{"harness":"grok","file":null,"state":"not managed","changes":[],"note":"grok reports to a Rex pane itself. For its usage, add to ~/.grok/config.toml: ..."}],"tmux":{"allow_passthrough":"on"}}
```

`replay` sends each pane's last status in tmux session `SESSION` again, for a tmux client that just attached and so missed it. In `tmux.conf`:

```bash
set-hook -g client-attached[7501] 'run-shell -b "sleep 0.3; rexboard hooks replay #{session_id}"'
set-hook -g client-session-changed[7501] 'run-shell -b "sleep 0.3; rexboard hooks replay #{session_id}"'
```

## `rexboard views`

```bash
rexboard views
```

The views `ls --view` and the board's `v` take, in the board's order: the built-in ones, then one per extension field. Each says what it groups by, or `one list`. With `--json`:

```json
[{"group_by":"status","name":"status"},{"group_by":null,"name":"table"},{"group_by":"harness","name":"harness"},{"group_by":"host","name":"host"}]
```

## `rexboard hosts`

```bash
rexboard hosts
```

The hosts serve watches, as the board's top bar sees them: this host as `local` and every `rex hosts` entry, each with its state (`starting`, `up`, `down` or `stopped`), how many agent rows it has, and why it is down or stopped, until it is up again. `down` retries by itself; `stopped` does not (see [Down and restart](#down-and-restart)). With `--json`:

```json
[{"error":null,"host":"local","rows":4,"state":"up"},{"error":"observer exited (exit status: 1): observer.lua:12: attempt to index a nil value","host":"mini","rows":0,"state":"down"}]
```

## `rexboard jump`, `next` and `pick`

```bash
rexboard jump mini/block:web-seat-b   # by the key from ls --json
rexboard jump 1                       # the first NEEDS YOU row
rexboard jump -                       # back to the pane before the last jump
rexboard next                         # the next pane that needs you
rexboard next --back                  # the other way
rexboard pick                         # choose from the board's list
```

Each lands the Rex app on the pane, on its host, and marks it seen through serve. A jump:

1. focuses the pane on the server that owns it (`session.focus_block`);
2. asks this Mac's Rex app to select the pane's session and tab (`client.perform session.select`). On this host that is the app client named by `REX_CLIENT`, else `client` in config.toml, else the only app connected over the unix socket. On a remote host it is the app connected from this Mac's Tailscale node.

The app has 5 seconds to answer, and it does not answer while the Mac's screen is locked. The jump then fails with `REX`, `the Rex app on NAME did not answer within 5s; it does not answer while the Mac's screen is locked`, and the next step `unlock the Mac, then rexboard jump KEY`. With `--json`, Rex's own words are in `error.detail`. [Configuration](configuration.md#which-window-a-jump-lands-in) says which app window a jump switches.

`next` picks from `needs_you` in board order: blocked, then error, then unseen done, each longest waiting first. It starts after the pane it runs in (`REX_BLOCK`), or outside a pane, after the pane the last jump went to (kept in `state.json`, so it survives a restart). It goes to unseen rows first, so repeated `next` works through the queue; once every row is seen, the blocked and erroring ones cycle.

`jump -` goes to the pane the jump before the last one went to, and the two swap, as `cd -` does, so a second `jump -` comes back. Every jump counts: `jump`, `next`, `pick`, and the board's `enter` and `-`.

`pick` opens the board's list without the turn card: Enter jumps and exits 0, esc exits 1. [TUI](tui.md#pick) has the rest.

`jump`, `next` and `pick` print `jumped to SESSION › TAB › PANE on HOST`; with `--json`, `{"ok":true,"key":...}`.

| Exit | When |
| --- | --- |
| 0 | Jumped |
| 1 | `NOBODY_NEEDS_YOU`: `next`, or `jump N`, with nothing in `needs_you`; prints `nobody needs you`. `NOT_FOUND`: no such key, no row N, or for `jump -`, `no previous jump yet` or `previous pane is gone`. `AMBIGUOUS_CLIENT`: no app client named, and none or more than one on the unix socket; the message says which, and the next steps name each app (`open the Rex app` when there is none). `CONFIG`: config.toml cannot be read or has an unknown key, or `client` names no app. `REX`: a step in Rex failed; the message names it. `pick`: esc |
| 2 | `USAGE`: TARGET is neither a key, a number nor `-`, or `pick` without a terminal |

## `rexboard ext`

```sh
rexboard ext ls                       # each extension found: contract, enabled, last run, dir
rexboard ext test my-tool             # run it once against serve's rows and check the answer
rexboard ext test my-tool --rows f    # against the rows in f (`rexboard ls --json --all > f`)
rexboard ext new my-tool              # start one from the example
```

`ext ls` never starts serve; without it, `LAST RUN` shows `-`. `ext test` exits 1 with `EXTENSION` when the answer breaks contract 1, when the run fails, or when there is no such extension, after printing the fields per row and every problem. `ext new` fails with `EXTENSION` when the directory is already there. A `NAME` that is not `[a-z0-9-]+`, or a `--rows` file that cannot be read as rows, fails with `USAGE` (exit 2), and a bad config.toml with `CONFIG`. [Extensions](extensions.md#commands) has the output of each, and the contract.

## `rexboard upgrade`

```bash
rexboard upgrade           # upgrade to the latest release, the way rexboard was installed
rexboard upgrade --check   # only say whether a newer release is out
```

`update` is another name for `upgrade`.

`upgrade` asks GitHub for the latest release (the tag `releases/latest` points to, through `curl`). When this rexboard is that version it prints `rexboard 0.9.0 is the latest` and changes nothing; a newer build says `rexboard 0.10.0 (the latest release is 0.9.0)`. When a newer release is out, `--check` prints `0.9.0 is out (you have 0.8.0): run rexboard upgrade`, and `upgrade` replaces the binary it runs from, found from its own path with links resolved:

- **Under a Homebrew `Cellar`:** `brew update --quiet`, then `brew upgrade vanducng/tap/rexboard`. brew refreshes its taps by itself only once a day, so without the update a release from today is not in the local tap yet. A failed update, such as one with no network, is a warning, and the upgrade goes on with the tap as it is. If that leaves the old version, the error says `brew update failed (its output is above), so the tap may be stale; rexboard is still 0.8.0`, with the next steps `brew update` and `rexboard upgrade`.
- **Anywhere else:** the release's `install.sh`, into the same directory (`REXBOARD_INSTALL_DIR`), which checks the archive against `SHA256SUMS`. It is fetched into a new private directory (`mktemp -d`), removed after.
- **A cargo build** (a `target` directory with `debug` or `release` under it), a `cargo install`, a binary not named `rexboard`, or a directory you cannot write: nothing runs. The command fails with `UPGRADE` and names what to run instead; for a directory you cannot write, that is the same installer with your own `sudo`. rexboard never runs `sudo` itself.

The output of `brew` or `install.sh` goes to stderr. Then the new binary is asked its version, and serve is stopped as `serve --stop` does: `upgraded rexboard 0.8.0 -> 0.9.0; serve stopped, the next command starts the new one; reopen an open board`. An open board keeps running the old version until you reopen it.

Right after a release, the Homebrew tap and the release archives can lag `releases/latest` by a few minutes. brew then leaves the old version (`the tap does not have 0.9.0 yet`, next step `rexboard upgrade`), and `install.sh` fails with `install.sh failed (its output is above); if 0.9.0 came out in the last few minutes, its archives may still be uploading, so retry then`, next step `rexboard upgrade`.

With `--json`, `--check` prints `{"ok":true,"current":"0.8.0","latest":"0.9.0","newer":true}`, and `upgrade` prints `{"ok":true,"from":"0.8.0","to":"0.9.0","upgraded":true,"method":"brew","serve_stopped":true}` (`method` is `brew` or `install.sh`; with nothing to do, `{"ok":true,"from":"0.9.0","to":"0.9.0","upgraded":false}`).

| Exit | When |
| --- | --- |
| 0 | Upgraded, already the latest, or checked |
| 1 | `UPGRADE`: the latest release could not be found, `brew` or `install.sh` failed, the binary still reports the old version afterwards, or this install is not one `upgrade` can replace; the next step says what to run |
