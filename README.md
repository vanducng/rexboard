# rexboard

An attention board for the coding agents running in your [Rex](https://www.superlogical.com/rex/docs) panes, across every Rex host: which one is working, blocked or done, for how long, and a jump to its pane.

<img alt="The rexboard board in a terminal: agent panes grouped into NEEDS YOU, WORKING, QUIET and UNREPORTED, one shape per state and a spinner on working rows. The selected row's card shows what its agent said; a permission prompt is answered from the board, and a pane opens live in the full view" src="docs/assets/demo.gif" width="100%">

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

Press `?` on the board for every key. What the board shows is also a command with `--json`, such as `rexboard ls --json`.

## How it works

<img alt="rexboard architecture: agents (Claude Code, Codex, pi, grok) report their state to Rex, on their own in a Rex pane or through a hook inside tmux. On this host and every host in rex hosts, Rex runs observer.lua, which streams NDJSON to rexboard serve; serve keeps state on disk, holds the harness adapters, runs the extensions (rows in, fields out) and answers the board, pick and the CLI over a unix socket. Three things write to Rex, each on one pane you chose, from the board or the CLI: a jump, an answer and a close act on the pane's host, and a jump then asks the Rex app to show the pane" src="docs/assets/architecture.svg" width="100%">

rexboard reads Rex and writes only to a pane you chose: a jump, an answer or a close. [System architecture](docs/system-architecture.md) has the details.

## Docs

- [TUI](docs/tui.md): the board, its views and keys, answer mode and `pick`
- [CLI](docs/cli.md): every command, its output and exit codes
- [Agent status](docs/agent-status.md): where a row's state comes from, and what to check when one looks wrong
- [Extensions](docs/extensions.md): add columns from any tool
- [Configuration](docs/configuration.md): `config.toml`
- [Decisions](docs/decisions/README.md): why rexboard works the way it does
- [Contributing](CONTRIBUTING.md): setup, checks and pull requests

## License

MIT. See [LICENSE](LICENSE).
