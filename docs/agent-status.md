# Agent status

rexboard shows the status Rex has for each pane. It never asks an agent anything and installs nothing into one, so a row is only as right as what reached Rex.

## Where status comes from

An agent reports its state (`working`, `blocked`, `done`, `idle` or `error`), with an optional `kind` and `msg`, by printing a status escape sequence (OSC 7501) into its pane. Rex keeps that record on the pane, and rexboard reads it. Each row's `source` says how the record got there:

| `source` | Where the agent runs | Where its record comes from |
| --- | --- | --- |
| `native` | Straight in a Rex pane | The agent itself. Claude Code and Codex report on their own there |
| `bridge` | Inside tmux, in a Rex pane that runs `tmux attach` (an OpenRig seat, for example) | A hook in the agent that sends the sequence through tmux, which would otherwise swallow it. A record counts as `bridge` when tmux wrote it or tmux is the pane's foreground process |
| `inferred` | Anywhere, with nothing reporting | No record. The state comes from the title's glyph (`✳ ` idle, a spinner working, else `unknown`), and the harness, if any, from the process (`claude` or a version such as `2.1.310` for Claude Code, `codex`, `pi`, `grok-…` for grok) or the title, so it ends in `?`. `AGE` is `-`, and the row sits under `UNREPORTED` |
| An extension's name | Anywhere, with nothing reporting | No record, but an enabled [extension](extensions.md) with `provides.status` gave the row a fallback status. The row is grouped by that state, like a reported one. A pane's own record always wins over it |

The turn card shows `source`, and `rexboard ls --json` has it on every row ([JSON fields](cli.md#json-fields)).

`msg` is whatever the reporter sent. Rex keeps `msg` on one line, so some hooks (the dotfiles ones, for example) join a message's lines with ` ⏎ `; rexboard splits them back. The `MSG` column shows the first line and the turn card the whole message. `MSG` is blank when there is no message, and for the filler a hook sends when it has no words: `Done`, `Working`, `Allow <tool>?`. `rexboard ls --json` keeps `msg` as sent.

An agent's own `done` record carries no message. A hook can add a sub-record with the id `turn` that carries the turn's final message, and rexboard uses its `msg` when the pane's own record has none. It is not counted as a child record. While the pane is `done` the `turn` message always applies, since the agent's own `done` lands just after it; in any other state it applies only when it is not older than the pane's record.

The reporting belongs to the agent's setup, not rexboard's. One working setup for agents in tmux seats is on the [Rex page of vanducng/dotfiles](https://dotfiles.vanducng.dev/rex/#agent-status): the hooks, `make rex-hooks-apply` to merge them into the agent's settings, and `REX_AGENT_STATUS_LOG` to log what each hook did.

## A row looks wrong

Start from the row's `source`.

| What you see | Why | What to do |
| --- | --- | --- |
| An agent you know is running shows `inferred`, with `AGE` `-`, under `UNREPORTED` | Nothing reported to Rex for that pane. Inside tmux, the agent's own sequence stops at tmux | Inside tmux, set up a bridge: Claude Code picks it up as it runs, and pi or Codex need a restart. Straight in a Rex pane, check that the agent is one that reports |
| A row lags its agent, or never leaves one state | Rex still has the agent's last report. For a `bridge` row, a hook usually did not run. A `working` report over 120 seconds old on a pane that looks idle shows as `idle?` ([TUI](tui.md#layout)) | For a `bridge` row, check that the hooks are installed and current: in the dotfiles setup, `make rex-hooks` prints what it would change and `make rex-hooks-apply` writes it. Then start the agent with `REX_AGENT_STATUS_LOG=FILE` and read which events arrive. The next report fixes the row |
| `MSG` is blank | The record has no message, or only filler | The reporter decides what it sends; see its setup. The turn card still shows the title |
| A dim row with `◌`, its host red in the top bar | That host's observer is down, so its rows are `stale` | `rex hosts`, then `serve.log` in the state dir ([Files](system-architecture.md#files)) |

To tell a Rex problem from a rexboard one, ask Rex directly. `rexboard ls --direct` reads Rex's snapshot without serve, and `rexboard watch --raw --host HOST` prints each status record as Rex sends it. If Rex has no record for the pane, the fix is in the agent's setup. If Rex has it and the board does not, that is a rexboard bug.
