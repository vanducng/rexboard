# rexboard

An attention board for the coding agents running in your [Rex](https://www.superlogical.com/rex/docs) panes, across every Rex host: which one is working, blocked or done, for how long, and a jump to its pane.

<img alt="The rexboard board in a terminal: agent panes numbered in a list-first board, grouped into NEEDS YOU, WORKING, QUIET and UNREPORTED, one shape per state and a spinner on working rows; c shows the selected pane's card and facts; a permission prompt is answered from the board, a pane opens live in the full view, and a message is sent to a seat from an extension action" src="docs/assets/demo.gif" width="100%">

## Install

Needs Rex, with its `rex` CLI on your PATH. Jumping to a pane also needs the Rex app.

```bash
brew install vanducng/tap/rexboard
```

Without Homebrew:

```bash
curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
```

## Quick start

```bash
rexboard
```

The board opens in this terminal. The first run starts `rexboard serve` in the background, which watches this host and every host in `rex hosts`. Rows that need you (blocked, an error, or done and not yet seen) are on top.

Agents straight in a Rex pane report on their own. Inside tmux they need hooks: run `rexboard hooks install` once (`--dry-run` first). See [Agent status](docs/agent-status.md).

| Key | Does |
| --- | --- |
| `j` / `k` | Move down / up |
| `enter` | Jump to the pane |
| `space` | Watch the pane live |
| `a` | Answer the agent |
| `m` | Mark it seen |
| `/` | Filter |
| `?` | Every key |
| `q` | Back, or quit |

## What you can do

- **Read the board.** Rows grouped by what needs you, with a turn card (`c`) under the list that shows what the agent said. [TUI](docs/tui.md)
- **Change the view.** `v` steps through status, table, harness, host, session and one view per extension field; `o` sorts by a column. [Views](docs/tui.md#views)
- **Filter.** `/` narrows by text, `f` by status. [Status filter](docs/tui.md#status-filter)
- **Mark rows and keep favorites.** `*` makes a pane a favorite, `'` goes to the marked rows, and `[[mark]]` rules in `config.toml` mark more. [Marked rows](docs/tui.md#marked-rows)
- **Keep groups.** `+` keeps a group and `f+` shows only the kept ones, so a board of many sessions shows the few you work in. [Kept groups](docs/tui.md#kept-groups)
- **Jump.** `enter` goes to a pane on whichever host owns it, `1`-`99` pick a row by its number, and `rexboard board --once` is a popup that closes when you land. [Digits](docs/tui.md#digits), [Popup](docs/tui.md#popup)
- **Answer and close.** `a` answers a dialog or an input line, `dd` closes a pane, `x` runs an extension's action. [Answer mode](docs/tui.md#answer-mode), [Actions](docs/tui.md#actions)
- **Script it.** Every command takes `--json`: `ls`, `watch`, `show`, `answer`, `close`, `jump`, `next`, `pick`. [CLI](docs/cli.md)
- **Add columns from any tool.** An extension reads rows and returns fields, marks and actions. [Extensions](docs/extensions.md)
- **Catch a decision.** In tmux, a `[hooks.ask]` rule makes a finished turn that asks for a decision show as blocked, and `/ask:` finds those rows. [Decision signals](docs/agent-status.md#decision-signals)
- **See what an agent spent.** `MODEL`, `CTX` and `EFFORT` show when an agent reports them, and `tui.json` adds `TOK`, `COST` and `5H`, all from `rexboard usage`. [Usage on every host](docs/agent-status.md#usage-on-every-host)
- **Wire up hooks.** `rexboard hooks install` puts the hook and usage commands into Claude Code, Codex and pi. [Setting it up](docs/agent-status.md#setting-it-up)

## How it works

<img alt="rexboard architecture: agents (Claude Code, Codex, pi, grok) report their state to Rex, on their own in a Rex pane, or, in tmux and in pi, through rexboard hook, and rexboard usage writes their usage records to the state dir. Rex runs observer.lua on this host and every host in rex hosts and streams NDJSON to rexboard serve. serve keeps state on disk, runs the extensions (rows in, fields out) and answers the board, pick and the CLI over a unix socket; an extension's action runs from a client. Writes go to one pane you chose: a jump, an answer and a close act through Rex on the pane's host, and a jump then asks the Rex app to show the pane" src="docs/assets/architecture.svg" width="100%">

rexboard reads Rex and writes to a pane you chose, through Rex: a jump, an answer or a close. It also writes its own state files, and the hook settings of the agents it names when you run `hooks install`. [System architecture](docs/system-architecture.md) has the details.

## Docs

- [TUI](docs/tui.md): the board, its views and keys, answer mode, `pick` and the popup
- [CLI](docs/cli.md): every command, its output and exit codes
- [Agent status](docs/agent-status.md): where a row's state comes from, decision signals, usage, and what to check when a row looks wrong
- [Extensions](docs/extensions.md): add columns from any tool
- [Configuration](docs/configuration.md): `config.toml`
- [System architecture](docs/system-architecture.md): the pieces, the files and what is written where
- [Decisions](docs/decisions/README.md): why rexboard works the way it does
- [Development](docs/development-guidelines.md): the code map and the tests
- [Deployment](docs/deployment.md): CI, releases and install
- [Contributing](CONTRIBUTING.md): setup, checks and pull requests

## License

MIT. See [LICENSE](LICENSE).
