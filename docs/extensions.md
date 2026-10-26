# Extensions

An extension adds fields to rows, and may give a row with no status of its own a fallback status, from any tool, without rexboard knowing the tool. It is a directory holding `extension.toml` and an adapter: a program that reads the rows as JSON on stdin and prints fields per row as JSON on stdout. rexboard speaks to it through **contract 1**, described here. This page is the contract's source of truth, and a test checks that its JSON samples parse with rexboard's own types.

## Write one

```sh
rexboard ext new my-tool        # ~/.config/rexboard/extensions/my-tool, from the example
$EDITOR ~/.config/rexboard/extensions/my-tool/adapter
rexboard ext test my-tool       # one run against serve's rows; exit 1 if the answer breaks the contract
```

Then add `my-tool` to `extensions.enabled` in [config.toml](configuration.md). serve reads the manifests and the `[extensions]` table when it starts and at each rescan (every 5 minutes); `rexboard serve --stop` picks up a change at once.

The example, [`examples/extensions/session-prefix`](../examples/extensions/session-prefix), is a whole adapter in POSIX `sh` and `jq`. It gives each row a `project`, its session label up to the first `-`:

```sh
#!/bin/sh
exec jq -c '
  (.settings.separator // "-") as $sep
  | {
      contract: 1,
      rows: (.rows | map({key, value: {fields: {project: (.session | split($sep)[0])}}}) | from_entries)
    }'
```

An adapter can be written in any language. It reads one request, prints one response and exits.

## The manifest

```toml
name = "session-prefix"  # [a-z0-9-]+, the directory's name
contract = 1             # the contract the adapter speaks
description = "A row's project: its session label up to the first separator"
cmd = ["./adapter"]      # argv, no shell; a leading ./ is the extension's directory
every = 10               # seconds between runs, at least 2
timeout = 5              # seconds, at least 1 and less than every; a slower run is killed
[[fields]]
name = "project"         # [a-z0-9-]+, stored on a row as session-prefix.project
title = "PROJECT"        # the column's header; keep it the name in capitals, so it says what to type
[provides]
status = false           # true lets it give rows with no record a status
```

`description`, `every`, `timeout`, `fields` and `provides` are optional; the values above for `every` and `timeout` are their defaults. A field needs both `name` and `title`. The extension is broken when the manifest has a key it does not know, a name that does not match the directory, a name or field name that is not `[a-z0-9-]+`, another contract, an empty `cmd`, `every` under 2, `timeout` of 0 or not less than `every`, or a field declared twice. `rexboard ext ls` says why, and serve skips it and logs why.

Extensions are found in `$XDG_CONFIG_HOME/rexboard/extensions` (`~/.config/rexboard/extensions`), then in each of `extensions.dirs`, one directory per extension. The first of a name wins; `ext ls` shows a later one as `broken: shadowed by an earlier extension named NAME`. Only the names in `extensions.enabled` run: a directory dropped in runs nothing by itself.

## The request

Every `every` seconds, serve runs the adapter in its directory and writes one request to its stdin:

```json request
{
  "contract": 1,
  "extension": "session-prefix",
  "settings": { "separator": "-" },
  "rows": [
    {
      "key": "mini/block:7f3a",
      "host": "mini",
      "session": "api-gateway",
      "tab": "main",
      "pane": "seat-a",
      "title": "✳ seat-a",
      "process": "claude",
      "harness": "claude-code",
      "state": "working",
      "source": "native"
    }
  ]
}
```

- `settings` is `[extensions.settings.NAME]` from config.toml, untouched, or `{}`.
- `rows` holds every pane serve has, plain shells too. The names match `rexboard ls --json`. `process` and `harness` may be `null`. A pane with no status record has `state` `unknown` and `source` `inferred`, or the state and `source` (the extension's name) of a fallback status given before.
- The adapter is not run until serve has rows.

## The response

The adapter prints one JSON object on stdout and exits 0:

```json response
{
  "contract": 1,
  "rows": {
    "mini/block:7f3a": {
      "fields": { "project": "api" },
      "status": { "state": "working", "kind": null, "msg": "Building the release" }
    }
  }
}
```

- `rows` is keyed by the request's `key`s. A row left out gets no fields.
- `fields` maps a declared field's name to a string, or to `null` for no value.
- `status` is optional, and only allowed when the manifest sets `provides.status = true`. `state` is one of `idle`, `working`, `blocked`, `done` and `error`. `kind` and `msg` are optional.

A row's fields reach `rexboard ls --json` under `ext`, by id: `"ext": {"session-prefix.project": "api"}`. A field's short name is its own (`project`), unless another enabled extension declares a field of the same name; then both go by id. On the board each field gets a view, a column and a filter key; see [TUI](tui.md#extension-fields).

A row's own status record always wins: an extension's status reaches only rows with none. Such a row has the extension's name as its `source`, and is grouped by the state given.

## Limits and failures

| What | Limit |
| --- | --- |
| stdout | 1 MiB |
| A field value | 128 characters |
| A status `kind` | 128 characters |
| A status `msg` | 2048 bytes |
| A run | `timeout` seconds, then the adapter and whatever it started are killed |

A run fails whole when the adapter exits with an error, times out, prints more than 1 MiB, prints something that is not JSON, answers for another contract, or leaves its stdout open after it exits. A failed run keeps the last good answer for 3 runs, then drops it.

Anything else wrong drops only that part, and the rest is kept:

- a row key that was not in the request (`unknown row KEY`);
- a field the manifest does not declare (`undeclared field NAME`);
- a key the contract does not have, at the top, in a row, or in a status (`unknown key NAME`);
- a value that is not a string, or is over its limit;
- a status without `provides.status`, or with a state not in the list.

serve logs each problem once to `serve.log` as `ext NAME: …`, with the first 4 KiB of the adapter's stderr per run. `rexboard ext test` prints them all.

## Trust

An extension runs with your permissions and sees every row's labels and titles, so enable only adapters you trust. Its `cmd` runs without a shell, in the extension's directory, in its own process group. Anything it leaves running when it exits, such as a process started with `&`, is stopped.

## Commands

### `rexboard ext ls`

One line per extension found: its name, contract, whether it is enabled, how its last run went, and its directory.

```
NAME            CONTRACT  ENABLED  LAST RUN                       DIR
session-prefix  1         yes      ok 41ms                        ~/.config/rexboard/extensions/session-prefix
my-tool         1         no       -                              ~/.config/rexboard/extensions/my-tool
old-tool        -         yes      broken: contract 2 is not ...  ~/.config/rexboard/extensions/old-tool
gone            -         yes      enabled, but not found         -
```

`LAST RUN` comes from serve, as `ok 41ms` or the run's error, and only when serve is already running: `ext ls` never starts it. A broken extension says why, and a name enabled in config.toml but found nowhere says so. `--json` prints an array with `name`, `contract`, `enabled`, `dir`, `last_run` (`{ok, error, at, ms}` or `null`) and `problem`.

### `rexboard ext test NAME [--rows FILE]`

Runs extension `NAME` once, enabled or not, with its manifest's timeout and its settings from config.toml. Then it prints the fields it gave each row and every problem with its answer:

```
my-tool (not enabled): answered in 38ms, fields for 2 of 5 rows
  mini/block:7f3a  project=api
  mini/block:9c21  project=web
problem: undeclared field owner
```

It sends serve's rows, starting serve when nothing answers, as `ls` does. `--rows FILE` sends the rows in FILE instead: an array as `rexboard ls --json` prints it, so `rexboard ls --json --all > rows.json` saves a set to test against. The adapter's stderr goes to stderr.

It exits 0 when the answer is valid. It exits 1 with `EXTENSION` when the answer breaks the contract, when the run fails (`my-tool: exited with exit status: 3`, `my-tool: timed out after 5s and was stopped`), when the extension is broken, or when there is none of that name. `--json` prints `{ok, extension, ms, sent, rows, problems}`.

### `rexboard ext new NAME`

Copies the example into `$XDG_CONFIG_HOME/rexboard/extensions/NAME`, with `NAME` as its name, and prints the next steps. `ext test NAME` passes on it unchanged. It fails with `EXTENSION` when the directory is already there, and with `USAGE` (exit 2) when `NAME` is not `[a-z0-9-]+`.
