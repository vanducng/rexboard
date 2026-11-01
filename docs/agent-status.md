# Agent status

rexboard shows the status Rex has for each pane. It never asks an agent anything, so a row is only as right as what reached Rex. The hooks that make an agent report are rexboard's own, but it installs them only when you run `rexboard hooks install` ([13](decisions/0013-rexboard-ships-the-hooks-it-relies-on.md)).

## Where status comes from

An agent reports its state (`working`, `blocked`, `done`, `idle` or `error`), with an optional `kind` and `msg`, by printing a status escape sequence (OSC 7501) into its pane. Rex keeps that record on the pane, and rexboard reads it. Each row's `source` says how the record got there:

| `source` | Where the agent runs | Where its record comes from |
| --- | --- | --- |
| `native` | Straight in a Rex pane | The agent itself. Claude Code and Codex report on their own there |
| `bridge` | Inside tmux, in a Rex pane that runs `tmux attach` (an OpenRig seat, for example) | A hook in the agent that sends the sequence through tmux, which would otherwise swallow it. A record counts as `bridge` when tmux wrote it, or tmux is the pane's foreground process, or the foreground is `/bin/sh` (the `sh -c ... tmux attach` wrapper OpenRig runs) |
| `inferred` | Anywhere, with nothing reporting | No record. The state comes from the title's glyph (`✳ ` idle, a spinner working, else `unknown`), and the harness, if any, from the process (`claude` or a version such as `2.1.310` for Claude Code, `codex`, `pi`, `grok-…` for grok) or the title, so it ends in `?`. `AGE` is `-`, and the row sits under `UNREPORTED` |
| An extension's name | Anywhere, with nothing reporting | No record, but an enabled [extension](extensions.md) with `provides.status` gave the row a fallback status. The row is grouped by that state, like a reported one. A pane's own record always wins over it |

The turn card shows `source`, and `rexboard ls --json` has it on every row ([JSON fields](cli.md#json-fields)).

`msg` is whatever the reporter sent. Rex keeps `msg` on one line, so some hooks (the dotfiles ones, for example) join a message's lines with ` ⏎ `; rexboard splits them back. The `MSG` column shows the first line and the turn card the whole message. `MSG` is blank when there is no message, and for the filler a hook sends when it has no words: `Done`, `Working`, `Allow <tool>?`. `rexboard ls --json` keeps `msg` as sent.

An agent's own `done` record carries no message. A hook can add a sub-record with the id `turn` that carries the turn's final message, and rexboard uses its `msg` when the pane's own record has none. It is not counted as a child record. While the pane is `done` the `turn` message always applies, since the agent's own `done` lands just after it; in any other state it applies only when it is not older than the pane's record.

## Usage on every host

`MODEL`, `EFFORT`, `CTX`, `TOK`, `COST` and `5H` come from what each agent's last turn used. On serve's own host the harness hands them to [`rexboard usage`](cli.md#rexboard-usage), which keeps a file per pane. Another host's files are not readable from here ([1](decisions/0001-reach-rex-only-through-the-rex-cli.md)), so the same call also tells Rex, in a second record on the pane next to the agent's own status:

- The record's `id` is `usage`. Rex drops keys it does not know from a status sequence, so the figures travel in the record's `msg`, as one line of JSON: `{"v":1,"m":"Opus 5.5","c":42,"w":200000,"t":84210,"u":1.2345,"q":17}`. `m` is the model (64 characters at most), `e` the reasoning effort as the harness says it (12 characters at most), `c` the context percent, `w` the window, `t` the prompt tokens of the last request, `u` the cost in dollars and `q` the five-hour quota percent. A figure the harness does not report is left out, never `0`. Nothing else leaves the host, and no one sees more than `MODEL`, `EFFORT`, `CTX`, `TOK`, `COST` and `5H` show. A model alias from `[models]` never travels: each host's serve applies its own table.
- It is sent only when the figures changed (as the record would say them: a cost below four decimals is not a change), at most once every 5 seconds per pane, and at once when a turn ends (a Codex `Stop` hook, pi's turn end), so the turn's last figures are not held back. A report counts as sent only once it has reached the pane's tty, and a burst of callbacks for one pane sends once. `SessionEnd` does not touch it: the record lasts while the pane lives, and a new session in the same pane replaces it with its own figures when they differ (the same figures are already on the row). A hook tells its status first, and tells Rex its usage only while it is still quick: slow tmux can cost a report to Rex, never the status or the pane's own usage file. Writing to a tty never waits more than a quarter of a second: a terminal that is full drops the report instead of holding the agent's status line. Inside tmux it also rides tmux passthrough and is kept on the tmux pane (`@rex_usage`), and `rexboard hooks replay` sends it again with the status when a client attaches later.
- Serve reads it with the rest of the pane. It is neither the pane's status nor a child record: a pane with only a `usage` record is still `inferred`. A pane's usage file on serve's own host wins over the record, whole; every other row has the record's figures.
- A pane keeps its last figures while it lives, as locally: Rex keeps the record until the pane goes, and each report replaces it.
- `REXBOARD_NO_USAGE_REPORT=1` in the agent's environment turns the sending off.
- A hook tells Rex its usage only when its own status took under 500 ms, so a slow tmux can cost a report but never the status inside the 5 s an agent gives a hook. The local usage file is kept either way.

A rexboard older than this does not know the record. It counts it as a child record of the pane (in `ls --json` only), and for a pane that has no status record of its own it takes the usage record for the pane's status: the row shows `done`, `native` and the raw JSON as its message, until the pane reports a status. Upgrade the host that runs serve first, then the others. A running agent picks the hooks up only when it next starts, so a host's panes show usage after its rexboard is upgraded and each agent has been started again; nothing here asks you to restart one.

## Setting it up

Straight in a Rex pane, Claude Code and Codex report on their own. Inside tmux they need hooks, and pi needs its extension everywhere:

```bash
rexboard hooks status                # what each harness has
rexboard hooks install --dry-run     # what an install would change
rexboard hooks install               # merge them in, after a backup
```

Inside tmux, also put `set -g allow-passthrough on` in `tmux.conf`, or the status stops at tmux. To check that the hooks run, start the agent with `REXBOARD_HOOK_LOG=FILE` and read which events arrive. Claude Code picks the hooks up as it runs; pi and Codex need a restart. Details, and how to undo, are in [`rexboard hooks`](cli.md#rexboard-hooks).

## A row looks wrong

Start from the row's `source`.

| What you see | Why | What to do |
| --- | --- | --- |
| An agent you know is running shows `inferred`, with `AGE` `-`, under `UNREPORTED` | Nothing reported to Rex for that pane. Inside tmux, the agent's own sequence stops at tmux | Inside tmux, run `rexboard hooks install` and make sure `tmux.conf` has `set -g allow-passthrough on` (`rexboard hooks status` says): Claude Code picks the hooks up as it runs, and pi or Codex need a restart. Straight in a Rex pane, check that the agent is one that reports |
| A row lags its agent, or never leaves one state | Rex still has the agent's last report. For a `bridge` row, a hook usually did not run. A `working` report over 120 seconds old on a pane that looks idle shows as `idle?` ([TUI](tui.md#layout)) | For a `bridge` row, check that the hooks are installed and current: `rexboard hooks status` says what an install would change. Then start the agent with `REXBOARD_HOOK_LOG=FILE` and read which events arrive. The next report fixes the row |
| `MSG` is blank | The record has no message, or only filler | The reporter decides what it sends; see its setup. The turn card still shows the title |
| A dim row with `◌`, its host red in the top bar | That host's observer is down, so its rows are `stale` | `rex hosts`, then `serve.log` in the state dir ([Files](system-architecture.md#files)) |

To tell a Rex problem from a rexboard one, ask Rex directly. `rexboard ls --direct` reads Rex's snapshot without serve, and `rexboard watch --raw --host HOST` prints each status record as Rex sends it. If Rex has no record for the pane, the fix is in the agent's setup. If Rex has it and the board does not, that is a rexboard bug.
