# CLI

`rexboard --help` is the full command contract. This page covers output, fields and exit codes.

## Finding Rex

rexboard runs the `rex` CLI. It uses the first of:

1. `$REXBOARD_REX`
2. `$REX_BIN_DIR/rex`
3. `rex` on `PATH`
4. the macOS app bundle: `~/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex`

With none of them, a command fails with `REX_NOT_FOUND`.

## `rexboard ls`

```bash
rexboard ls                 # agent panes on every host, most urgent first
rexboard ls --all           # plain shells too
rexboard ls --host mini     # one host: local, or a label from `rex hosts`
rexboard ls --json          # an array of panes
```

One embedded Lua script runs through `rex do -` and reads this host plus every `rex hosts` entry in one call. A call that takes more than 10 seconds fails with `REX`.

A pane is listed when it has a status record or a known harness. `--all` adds the rest. Panes sort by state: `blocked`, `error`, `done`, `working`, `idle`, `unknown`.

The text table has the columns `STATE`, `AGE` (since `updated_at`, `-` when there is none), `HARNESS`, `HOST`, `WHERE` (`session › tab › pane`) and `WHAT` (the record's message, else the pane title).

### JSON fields

`--json` prints one array. Each pane has every field below; a missing value is `null`.

| Field | Type | Meaning |
| --- | --- | --- |
| `host` | string | `local`, or the label from `rex hosts` |
| `session_id` | string | Rex session id |
| `session` | string | Session label |
| `window_id` | string | Rex window id |
| `tab` | string | Window (tab) label |
| `block_id` | string | Rex block id of the pane |
| `pane` | string | Block (pane) label |
| `title` | string | Terminal title, `""` when none |
| `harness` | string or null | `effective_app` (else `app`) of the record, else the foreground process (`claude`, `codex`, `pi`; a version string like `2.1.310` is `claude-code`), else `claude-code` for a title that starts with a Claude state glyph |
| `state` | string | `blocked`, `error`, `done`, `working` or `idle` from the record; with no record, from the title glyph (`✳ ` is `idle`, a braille spinner or `◐`/`◑` is `working`), else `unknown` |
| `kind` | string or null | The record's `kind`, such as `permission` |
| `msg` | string or null | The record's message |
| `updated_at` | string or null | RFC 3339 time Rex received the record. It changes on every report, not only on a state change |
| `source` | string | `native`: the pane reports its own status. `bridge`: the record came through `tmux`. `inferred`: no record, state read from the title |
| `children` | object | Counts by state of the pane's other records, such as Claude subagents: `{"working": 1}` |

The pane's main record is the one with no `id`; if every record has one, the most urgent wins.

Fields are a contract. New fields may appear. Renaming or removing one is a breaking change.

### Exit codes and errors

| Exit | When |
| --- | --- |
| 0 | Listed. A host that cannot be reached prints `warning: HOST is unreachable: ...` on stderr, and the rest are listed |
| 1 | `REX_NOT_FOUND`: no `rex` CLI. `REX`: `rex do` failed, timed out, or printed something unexpected |
| 2 | `HOSTS_UNREACHABLE`: every host failed. `USAGE`: a bad flag, or `--host` names no host |

With `--json`, errors print on stderr as `{"ok":false,"error":{"code":...,"message":...,"next_steps":[...]}}`.
