# rexboard

An attention board for coding-agent panes across [Rex](https://www.superlogical.com/rex/docs) hosts: which agent is working, blocked or done, for how long, and a jump to its pane.

<img alt="The rexboard board in a terminal: seven agent panes on demo sessions in NEEDS YOU, WORKING, QUIET and UNREPORTED, with a PROJECT column from an example extension, braille spinners on working rows and one shape per state. A done row's card shows its message and m marks it seen; a on the permission row opens answer mode over the dialog, pressing 2 once asks for a second press, 1 picks Yes and the card says sent 1 (Yes); z lists the keys that may follow it and za folds a group and opens it again; /docs filters to the docs row, whose live full view follows its coloured diff until it reports done; q goes back, and dd asks before y closes the finished pane" src="docs/assets/demo.gif" width="100%">

## Install

Needs Rex, with its `rex` CLI on your PATH. Jumps also need the Rex app.

```bash
brew install vanducng/tap/rexboard
```

## Quick start

```bash
rexboard                 # the board, in this terminal
rexboard next            # go to the next pane that needs you
rexboard pick            # choose a pane from a list and go to it
rexboard ls --json       # every agent pane, most urgent first
rexboard upgrade         # upgrade to the latest release
```

The first command starts `rexboard serve` in the background. It watches this host and every host in `rex hosts`, and remembers what Rex does not: how long each pane has been in its state, and whether you have seen it.

On the board, `j`/`k` move, `n` goes to the next row that needs you, `enter` jumps to its pane and `-` comes back. `space` watches the pane live, `a` answers it, `m` marks it seen, `/` filters, `o` sorts and `dd` closes a pane after asking. `?` lists every key. The keys are vim's and work in a stock Rex window. See [TUI](docs/tui.md#keys).

A pane needs you when it is blocked, has an error, or is done and you have not seen it. Going to it, or `m`, marks it seen. Its state is what its agent reports to Rex: Claude Code and Codex report on their own in a Rex pane, an agent inside tmux needs a hook, and a pane that reports nothing is read from its title. [Agent status](docs/agent-status.md) says what to check when a row looks wrong.

rexboard only reads Rex, except to jump to a pane and to answer or close one, from the board or the CLI.

## Docs

| Page | What it covers |
| --- | --- |
| [CLI](docs/cli.md) | Every command, its output, JSON fields and exit codes |
| [TUI](docs/tui.md) | The board's layout, views, keys, full view, answer mode and `pick` |
| [Agent status](docs/agent-status.md) | Where each row's status comes from, and what to check when one looks wrong |
| [Configuration](docs/configuration.md) | `config.toml`: the app client, excluded hosts and extensions |
| [Extensions](docs/extensions.md) | Add fields, and a fallback status, from any tool |
| [System architecture](docs/system-architecture.md) | serve, the observers, the Rex calls, the protocol and the files |
| [Decisions](docs/decisions/README.md) | Why rexboard works the way it does, one record per decision |
| [Contributing](CONTRIBUTING.md) | Setup, checks, how to add a key, command or page, and pull request rules |
| [Development](docs/development-guidelines.md) | The code map and the tests |
| [Deployment](docs/deployment.md) | CI, releases, install, upgrade and rollback |

## License

MIT. See [LICENSE](LICENSE).
