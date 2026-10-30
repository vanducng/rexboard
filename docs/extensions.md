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
icon = "\uF487"          # one character, one cell wide: the extension's mark. Not drawn on the card, which has no extension row; `ext ls --json` lists it
[[fields]]
name = "project"         # [a-z0-9-]+, stored on a row as session-prefix.project
title = "PROJECT"        # the column's header; keep it the name in capitals, so it says what to type
icon = "\uF502"          # the field's own mark on the card, in place of its name
[provides]
status = false           # true lets it give rows with no record a status
```

A field may also have `hidden = true`: its value reaches the row (`ls --json`, an action's placeholders and `when`) with no column, view, filter key or card fact. An extension may add `[[actions]]` tables: see [Actions](#actions).

`description`, `every`, `timeout`, `icon`, `fields` (and each field's `icon`) and `provides` are optional; the values above for `every` and `timeout` are their defaults. A field needs both `name` and `title`. The extension is broken when the manifest has a key it does not know, a name that does not match the directory, a name or field name that is not `[a-z0-9-]+`, another contract, an empty `cmd`, `every` under 2, `timeout` of 0 or not less than `every`, a field declared twice, or an `icon` that is not one character one cell wide. `rexboard ext ls` says why, and serve skips it and logs why.

The icons show only with `"icons": "nerd"` (see [TUI](tui.md#icons)): the card labels each of the extension's fields with the field's `icon`, in place of its name. A field with no `icon` keeps its name. The extension's own `icon` is not drawn on the card, which has no extension row. In words mode, and everywhere else (column headers, `ls`, `--json`), titles and names are used as before; `ext ls --json` lists an extension's `icon`. A Nerd Font glyph from the Basic Multilingual Plane (U+E000 to U+F8FF) is the safe choice: one from plane 15 can draw two cells wide in some terminals. A rexboard older than this key does not know it, so it marks a manifest that has one broken.

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

## Actions

An extension can offer something to do with a row, such as sending a message to the seat in it. It declares the action; rexboard asks, checks and runs it ([decision 16](decisions/0016-extensions-can-offer-actions.md)).

```toml
[[fields]]
name = "session"
title = "SESSION"
hidden = true            # the adapter gives it; no column

[[actions]]
name = "send"            # [a-z0-9-]+, unique in the extension
title = "Send message"   # the input line's label, and the word `x` lists
key = "s"                # one printable character, after the board's x; unique in the extension
cmd = ["./send", "--", "{field:session}", "{text}"]   # argv, no shell; it must hold {text}
confirm = "send to {field:session} on {host}: {text}" # the red line before y; default: TITLE: TEXT?
when = ["session"]       # the action applies to a row with a value for each of these fields
max = 1000               # bytes of text, 1 to 4096
timeout = 10             # seconds, 1 to 60; the run and what it started are then killed
```

`confirm`, `when`, `max` and `timeout` are optional; the values above for the last two are their defaults. `cmd` and `confirm` take `{host}` (the row's host), `{field:NAME}` (a declared field of this extension, which the row must have a value for) and `{text}`, the one line the user types. Each placeholder is expanded inside one argv element, so the text can hold quotes, `;`, `$(…)` or a leading `-` and still arrives as one argument; put `--` before it if the program reads options. The program (the first element) cannot be a placeholder. The text is one line with no control characters and at most `max` bytes. The manifest is broken when an action breaks any of this, repeats a name or a key, or names a field it does not declare.

The action runs in the extension's directory with no stdin, in its own process group, with `REXBOARD_EXTENSION`, `REXBOARD_ACTION` and `REXBOARD_SETTINGS` (its `[extensions.settings]` as JSON) in the environment. It succeeds on exit 0, and what it printed first is the result. A non-zero exit, a timeout or a program that cannot start is a failure that says so. rexboard keeps the first 4 KiB of each output.

`rexboard ext run NAME ACTION KEY --text TEXT [--dry-run | --yes]` runs it from the command line.

## Trust

An extension runs with your permissions and sees every row's labels and titles, so enable only adapters you trust. Its actions run the same way, after you confirm them. Its `cmd` runs without a shell, in the extension's directory, in its own process group. Anything it leaves running when it exits, such as a process started with `&`, is stopped.

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

`LAST RUN` comes from serve, as `ok 41ms` or the run's error, and only when serve is already running: `ext ls` never starts it. A broken extension says why, and a name enabled in config.toml but found nowhere says so. `--json` prints an array with `name`, `contract`, `enabled`, `dir`, `last_run` (`{ok, error, at, ms}` or `null`), `problem`, `icon` and `actions` (their names).

### `rexboard ext test NAME [--rows FILE]`

Runs extension `NAME` once, enabled or not, with its manifest's timeout and its settings from config.toml. Then it prints the fields it gave each row and every problem with its answer:

```
my-tool (not enabled): answered in 38ms, fields for 2 of 5 rows
  mini/block:7f3a  project=api
  mini/block:9c21  project=web
problem: undeclared field owner
```

It sends serve's rows, starting serve when nothing answers, as `ls` does. `--rows FILE` sends the rows in FILE instead: an array as `rexboard ls --json` prints it, so `rexboard ls --json --all > rows.json` saves a set to test against. The adapter's stderr goes to stderr.

It exits 0 when the answer is valid. It exits 1 with `EXTENSION` when the answer breaks the contract, when the run fails (`my-tool: exited with exit status: 3`, `my-tool: timed out after 5s and was stopped`), when the extension is broken, or when there is none of that name. `--json` prints `{ok, extension, ms, sent, rows, problems, actions}`. It lists the extension's actions after the rows and checks each without running it: the program must be an executable file, and the first row the action applies to must fill every placeholder (`problem: action send: on mini/seat-a: this row has no session`). With no such row it says so on stderr and passes. A problem there exits 1 with `EXTENSION`, `acts's actions have 1 problem`.

### `rexboard ext run NAME ACTION KEY --text TEXT [--dry-run | --yes] [--rows FILE]`

Runs extension `NAME`'s action `ACTION` on the row `KEY` (`host/block`, as `rexboard ls` prints it), with the checks the board makes: the row must have a value for each field in `when`, the text must be one line within `max`. Rows come from serve, which has the extension's fields only while the extension is enabled; `--rows FILE` reads them from a file as `ext test` does.

- `--dry-run` prints the confirm line and the argv, and runs nothing. With `--json`: `{ok, extension, action, key, argv, confirm, dry_run: true, output: null}`.
- `--yes` runs it. On a terminal, without `--yes`, it prints the confirm line and `[y/N]` on stderr and runs on `y`. With no terminal and neither flag it fails and runs nothing.
- It prints `TITLE: FIRST LINE` (`done` when the run printed none), or with `--json` the object above with `dry_run: false` and `output`.

It exits 1 with `EXTENSION` when the extension, action or row is not there, the action does not apply, the text is refused, the run fails or times out, or the answer was not `y`. SIGINT, SIGTERM and SIGHUP while it runs kill the action and what it started, then exit 1 (`interrupted and was stopped`): the action is in its own process group, so the terminal's Ctrl-C would not reach it.

### `rexboard ext new NAME`

Copies the example into `$XDG_CONFIG_HOME/rexboard/extensions/NAME`, with `NAME` as its name, and prints the next steps. `ext test NAME` passes on it unchanged. It fails with `EXTENSION` when the directory is already there, and with `USAGE` (exit 2) when `NAME` is not `[a-z0-9-]+`.
