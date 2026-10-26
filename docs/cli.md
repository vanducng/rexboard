# CLI

`rexboard --help` is the full command contract. This page covers output, fields and exit codes.

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
```

`ls` asks [`rexboard serve`](#rexboard-serve) for its rows and starts it when nothing answers, so the rows carry the history serve has kept: `since`, `seen` and the turn fields. `--direct` skips serve: one embedded Lua script runs through `rex do -` and reads this host plus every `rex hosts` entry in one call. A call that takes more than 10 seconds fails with `REX`.

A pane is listed when it has a status record or a known harness. `--all` adds the rest. `ls` builds its rows the same way as `watch --json`, so they carry the same fields and order: by `group`, then, inside `needs_you`, `blocked` before `error` before unseen `done`, then the longest in its state first. `ls --direct` sees one snapshot, so every `done` is unseen and `last_turn` is `null`.

The text table has the columns `GROUP`, `STATE`, `AGE` (time in the current state as `34s`, `12m`, `5h43m` or `3d4h`; `≥` when estimated; `-` for a pane with no status record, whose time cannot be known), `HARNESS` (`claude`, `codex` and so on, with a `?` when it is read from the title of a pane with no status record; `-` when unknown), `HOST`, `WHERE` (`session › tab › pane`, each label cut to 24 columns with `…` in the middle, so a long OpenRig tab label keeps both ends; a pane labelled `shell`, or named after its foreground process, tells you nothing, so it shows as `session › tab`) and `WHAT` (the record's message, else the pane title without its leading state glyph). In a terminal, `WHAT` and then `WHERE` are cut so each line fits its width. `WHERE` is cut a long tab first (one cut below 6 columns shows as `…` alone), then the session down to an 8-column stem such as `api-…way`, then the last label down to 6 columns; only then do a short tab and the stems give way. The ` › ` separators always stay. Rows that would still read the same end in `#N`, the pane's place among its tab's agent panes, oldest first. Piped output keeps every column whole. Every cell is printed without escape sequences or control characters, since titles and messages come from the programs in each pane. Widths count wide characters such as CJK and emoji as two columns. `--json` keeps every label and title whole, and the full harness name (`claude-code`).

### JSON fields

`--json` prints one array. Each pane has every field below; a missing value is `null`.

| Field | Type | Meaning |
| --- | --- | --- |
| `key` | string | `HOST/BLOCK_ID`, what `rexboard seen` takes |
| `host` | string | `local`, or the label from `rex hosts` |
| `session_id` | string | Rex session id |
| `session` | string | Session label |
| `window_id` | string | Rex window id |
| `tab` | string | Window (tab) label |
| `block_id` | string | Rex block id of the pane |
| `pane` | string | Block (pane) label |
| `title` | string | Terminal title, `""` when none |
| `process` | string or null | The pane's foreground process, as in `zsh`, `tmux`, or a version such as `2.1.310` (Claude Code runs under its version number) |
| `harness` | string or null | `effective_app` (else `app`) of the record, else the foreground process (`claude`, `codex`, `pi`; a version string like `2.1.310` is `claude-code`), else `claude-code` for a title that starts with a Claude state glyph, else `codex` for a tmux pane titled `DIR | MODEL` (exactly one ` | `, a `DIR` with no spaces, a `MODEL` of letters, digits, `.` and `-` with at least one letter and one digit) |
| `state` | string | `blocked`, `error`, `done`, `working` or `idle` from the record; with no record, from the title glyph (`✳ ` is `idle`, a braille spinner or `◐`/`◑` is `working`), else `unknown` |
| `kind` | string or null | The record's `kind`, such as `permission` |
| `msg` | string or null | The record's message |
| `updated_at` | string or null | RFC 3339 time Rex received the record. It changes on every report, not only on a state change |
| `source` | string | `native`: the pane reports its own status. `bridge`: the record came through `tmux`. `inferred`: no record, state read from the title |
| `children` | object | Counts by state of the pane's other records, such as Claude subagents: `{"working": 1}` |
| `since` | string | RFC 3339 UTC time the pane entered its current `state` and `kind`. A report in the same state keeps it |
| `since_estimated` | boolean | `true` when rexboard did not see the change: a row first seen in a snapshot that is `working` (its `updated_at` is the last report, not the start) or has no record, or an `updated_at` that is not RFC 3339. That last case also prints `warning: HOST: updated_at "VALUE" is not RFC 3339 (Rex VERSION); time in state is estimated` on stderr, once per host. A row with no record still carries a `since` in `--json`; the text table and the board show its age as `-` |
| `turn_started` | string or null | When the pane last entered `working` |
| `last_turn` | integer or null | Seconds from entering `working` to the next `done` or `blocked`. Short when the turn was already running when rexboard started |
| `seen` | boolean | `false` after the pane enters `done`, `blocked` or `error`; `true` once it enters `working` again. A `done`, `blocked` or `error` first seen in a snapshot starts unseen. `rexboard seen KEY`, or `s` on the board, marks it seen |
| `stale` | boolean | `true` while the host's observer is down; the next snapshot clears it |
| `suspect` | boolean | `true` when the record says `working`, its last report is more than 120 seconds old, and the pane disagrees: its title starts with Claude's idle `✳ ` (not checked when `tmux` is in front, since the title is then tmux's and keeps the idle glyph while the agent works), or a shell (`zsh`, `bash`, `fish`, `sh`) is back in front. A hook that never reported the end of a turn leaves this behind. `state` keeps the record's value; the board and the `ls` table show `idle?` |
| `suspect_reason` | string or null | Why, as in `reported working at 14:02; the title shows idle`. A report a day old or more gives its date: `reported working at Oct 24 14:02; zsh is back in front` |
| `group` | string | `needs_you` (`blocked`, `error`, unseen `done`), `working`, `quiet` (`idle`, seen `done`), or `unreported` (`source` is `inferred`, `state` is `unknown`, `stale`, or `suspect`) |

The pane's main record is the one with no `id`; if every record has one, the most urgent wins.

Fields are a contract. New fields may appear. Renaming or removing one is a breaking change.

### Exit codes and errors

| Exit | When |
| --- | --- |
| 0 | Listed. A host that cannot be reached prints `warning: HOST is unreachable: ...` on stderr (through serve: one line per host that is `starting`, `down` or `stopped`), and the rest are listed. When a host is `down` or `stopped`, the warning and the `HOSTS_UNREACHABLE` error end with `; see STATE_DIR/serve.log` |
| 1 | `REX_NOT_FOUND`: no `rex` CLI. `REX`: `rex do` failed, timed out, or returned a snapshot rexboard does not understand; the message names the Rex version. `SERVE`: serve did not start or answer; the message names `serve.log` |
| 2 | `HOSTS_UNREACHABLE`: every host failed; next steps `rex hosts` and `rexboard ls`. `USAGE`: a bad flag, or `--host` names no host |

With `--json`, errors print on stderr as `{"ok":false,"error":{"code":...,"message":...,"next_steps":[...]}}`.

## `rexboard watch`

```bash
rexboard watch --json               # the agent rows from every host, again after each change
rexboard watch --json --direct      # the same rows from this command's own observers
rexboard watch --raw                # every event from every host, as it happens
rexboard watch --raw --host mini    # one host: local, or a label from `rex hosts`
rexboard watch --raw --for 60       # stop after 60 seconds
```

`watch` needs `--json` or `--raw` for now; with neither, it fails with `USAGE`. With both, `--raw` wins and errors print as JSON.

`watch --json` subscribes to [`rexboard serve`](#rexboard-serve), starting it when nothing answers, and exits 1 with `SERVE` if serve stops. `--direct`, and `--raw` always, run this command's own observers instead: rexboard reads the host list once from `rex hosts --json`, adds this host as `local`, and keeps one embedded Lua observer running per host through `rex [-S HOST] do - --all --fail-fast`. A host added after a direct `watch` starts needs a restart. Without `--for`, `watch` runs until Ctrl-C, SIGTERM, or until stdout closes.

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
| `process` | `session_id`, `block_id`, `name`, `cwd` | The foreground process changed |
| `child_exited` | `session_id`, `block_id`, `exit_code` | The pane's process exited |
| `block_created` | `session_id`, `window_id`, `block` | A terminal pane opened. `block` has the same shape as a block in `snapshot` |
| `block_closed` | `session_id`, `block_id` | A pane closed |
| `window_created` | `session_id`, `window_id`, `label` | A tab opened |
| `window_closed` | `session_id`, `window_id` | A tab closed |
| `label` | `kind`, `session_id`, `id`, `label` | A session or tab was renamed. `kind` is `session` or `window`; `id` is that session or window id |
| `active_window` | `session_id`, `window_id` | The focused tab changed. `window_id` is `null` when the session's last tab closed |
| `session_created` | `session` | A session started. `session` is the full entry, the same shape as one session in `snapshot`: `session_id`, `label`, and `windows` with their blocks. A pane's title and process can still be empty here; `title` and `process` lines follow |
| `session_destroyed` | `session_id` | A session ended |
| `tick` | none | Every 30 seconds per host, so a quiet host still shows it is alive |
| `down` | `reason`, `retry_in_s` | rexboard lost the observer. See below |

A line of a type not listed here is passed through as Rex sent it, with `host` and `v` added. Safe to ignore.

Types may be added. Renaming or removing a type or field is a breaking change.

### Down and restart

When an observer exits, its output can't be read, or it prints nothing for 90 seconds (three missed ticks), rexboard prints a `down` line, waits `retry_in_s` seconds, and starts a new observer. The new observer begins with a fresh `snapshot`. A Lua handler that fails also ends the observer (`--fail-fast`), and the last line it printed on stderr is added to `reason`:

```json
{"host":"mini","reason":"observer exited (exit status: 1): observer.lua:12: attempt to index a nil value","retry_in_s":1,"type":"down","v":1}
```

The wait starts at 1 second and doubles up to 30. It goes back to 1 only after a run that lasted at least 30 seconds, so an observer that fails on every event backs off instead of restarting every second. A host that keeps going down never stops `watch`.

A line rexboard cannot read restarts that host's observer the same way: a line that is not JSON, a known type with a bad field, or (with `--json` and in `serve`) a record the board rejects, such as a status without `state`. stderr gets `warning: HOST: ERROR; restarting its observer: LINE`, with the line cut at 500 characters. Until the new snapshot the host's rows are `stale` and can lag: about 1 second after a one-off line. If every snapshot carries the same line, the host stays `stale`, retries with a wait that doubles up to 30 seconds, and logs the warning each time. Only a different `v` (`CONTRACT_MISMATCH`) stops a host for good: `warning: HOST stopped and will not restart: ...`, and the other hosts keep streaming.

### Exit codes and errors

| Exit | When |
| --- | --- |
| 0 | `--for` elapsed, Ctrl-C or SIGTERM, or stdout closed |
| 1 | `REX_NOT_FOUND`: no `rex` CLI. `REX`: `rex hosts` failed. `CONTRACT_MISMATCH`: every host stopped and the last one stopped on a version mismatch; next steps `rexboard --version` and `brew upgrade rexboard` |
| 2 | `USAGE`: neither `--json` nor `--raw`, a bad flag, or `--host` names no host |

## `rexboard board`

```bash
rexboard          # in a terminal, with no command
rexboard board
```

The board: the agent panes `ls` lists, grouped as `NEEDS YOU`, `WORKING`, `QUIET` and `UNREPORTED`, with a ticking time in state. It is a client of [`rexboard serve`](#rexboard-serve) and starts it when nothing answers. `board` without a terminal fails with `USAGE`; [TUI](tui.md) has the layout and keys.

## `rexboard serve`

```bash
rexboard serve                # start it in the background, or say it is already running
rexboard serve --foreground   # run it in this terminal
rexboard serve --stop         # stop it and its observers
```

`serve` owns one observer per host and the board, and answers `ls`, `watch --json` and `seen` over a unix socket. Any of those starts it when nothing answers, as `rexboard serve --foreground` in its own session with its output in `serve.log`, and waits up to 2 seconds for it. The socket, `serve.sock`, sits beside `serve.lock` in the state dir, so every caller finds the running serve whatever its `TMPDIR` or `XDG_RUNTIME_DIR`. If the path is longer than 103 bytes, commands fail with `SERVE`: set a shorter `XDG_STATE_HOME` everywhere rexboard runs (it also holds `state.json`). serve keeps the environment of the command that started it. After changing `REXBOARD_REX` or `REX_BIN_DIR`, run `rexboard serve --stop`. Only one runs at a time: a second `serve --foreground` fails with `SERVE_RUNNING` (exit 1). It rescans `rex hosts` every 5 minutes.

`serve` keeps `since`, `seen` and the turn fields in `state.json` and puts them back after a restart for every pane whose `state` and `kind` did not change. It appends each state change to `transitions.jsonl`. [System architecture](system-architecture.md) has the protocol and the files.

**Upgrades take effect by themselves.** Every command that talks to serve (`ls`, `watch`, `seen`, `next`, the board, and `serve` itself) first asks serve which rexboard it is. When serve is older, the command stops it, starts its own, and says so once, for example `restarted serve 0.3.0 or older -> 0.4.0`. A serve from 0.3.0 or older does not know the question, which counts as older. A newer serve, or one whose version it cannot read, is kept, so two installed versions never take turns restarting it. A serve that holds `serve.lock` but does not answer, such as one from 0.2.0 or older listening under its own `TMPDIR`, or a stuck one, is healed the same way when it is a `rexboard` process: `restarted serve (pid N did not answer) -> 0.4.0`. Only a holder that was there before the command's 2-second wait, and still is after it, is stopped, so a serve that is still starting is never touched. Commands that find the same old serve at once take turns, and only the first restarts it. The line goes to stderr and to `serve.log`; from the board it goes only to `serve.log`, so the screen stays whole. A holder that is not rexboard is left alone, and the command fails with `SERVE`, saying whether the lock names no pid, a pid that is not running, or one that is not rexboard, with `lsof` and `kill` as the next steps.

`serve --stop` exits 0 whether or not serve was running. A serve that holds the lock but does not answer gets SIGTERM, but only when the pid in `serve.lock` is a `rexboard` process: by its executable's path, or by its process name when the path cannot be read, as on macOS once an upgrade has deleted the binary it runs. A serve that ignores SIGTERM is named with `kill -9 N` as the next step. It waits up to 5 seconds for serve's process to exit, then fails with `SERVE`. `serve --json` prints `pid` as a number, `null` when unknown.

## `rexboard seen`

```bash
rexboard seen mini/block:web-seat-b
```

Marks a pane seen, by the `key` from `ls --json`: it leaves `needs_you` if it was an unseen `done`. serve keeps it across restarts until the pane changes state again. An unknown key fails with `NOT_FOUND` (exit 1). A key without `/` fails with `USAGE` (exit 2).

## `rexboard jump`, `next` and `pick`

```bash
rexboard jump mini/block:web-seat-b   # by the key from ls --json
rexboard jump 1                       # the first NEEDS YOU row
rexboard next                         # the next pane that needs you
rexboard next --back                  # the other way
rexboard pick                         # choose from the board's list
```

Each lands the Rex app on the pane, on its host, and marks it seen through serve, as `s` on the board does. A jump:

1. focuses the pane on the server that owns it (`session.focus_block`);
2. asks this Mac's Rex app to select the pane's session and tab (`client.perform session.select`). On this host that is the app client named by `REX_CLIENT`, else `client` in config.toml, else the only app connected over the unix socket. On a remote host it is the app connected from this Mac's Tailscale node. The app has 5 seconds to answer. It does not answer while the Mac's screen is locked; the jump then fails with `REX`, `the Rex app on NAME did not answer within 5s; it does not answer while the Mac's screen is locked`, and the next step `unlock the Mac, then rexboard jump KEY`. With `--json`, Rex's own words are in `error.detail`.

`next` picks from `needs_you` in board order: blocked, then error, then unseen done, each longest waiting first. It starts after the pane it runs in (`REX_BLOCK`); outside a pane, after the pane the last jump went to (serve keeps that in memory, so a restart starts from the top). It goes to unseen rows first, so repeated `next` works through the queue; once every row is seen, the blocked and erroring ones cycle. `pick` opens the board's list without the detail pane: Enter jumps and exits 0, esc exits 1.

`jump` and `next` print `jumped to SESSION › TAB › PANE on HOST`; with `--json`, `{"ok":true,"key":...}`.

| Exit | When |
| --- | --- |
| 0 | Jumped |
| 1 | `NOBODY_NEEDS_YOU`: `next`, or `jump N`, with nothing in `needs_you`; prints `nobody needs you`. `NOT_FOUND`: no such key, or no row N. `AMBIGUOUS_CLIENT`: no app client named, and no app connected, none on the unix socket, or more than one there; the message says which, and the next steps name each app (`open the Rex app` when there is none). `CONFIG`: config.toml cannot be read or has an unknown key, or `client` names no app. `REX`: a step in Rex failed; the message names it. `pick`: esc |
| 2 | `USAGE`: TARGET is neither a key nor a number, or `pick` without a terminal |
