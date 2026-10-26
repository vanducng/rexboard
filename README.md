# rexboard

An attention board for coding-agent panes across [Rex](https://www.superlogical.com/rex/docs) hosts: which agent is working, blocked or done, for how long, and a jump to its pane.

<img alt="The rexboard board in a terminal: agent panes on demo sessions move from WORKING to NEEDS YOU to QUIET while their times in state tick; a peek, marking a pane seen, the key table and a filter are shown" src="docs/assets/demo.gif" width="100%">

## Install

Needs Rex, with its `rex` CLI on your PATH. Jumps and `rexboard window` also need the Rex app.

```bash
curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
```

This installs the latest release for macOS on Apple silicon or Linux x86_64 into `~/.local/bin`, after checking it against the release's `SHA256SUMS`. Or build from source with Rust 1.88 or newer:

```bash
cargo install --locked --git https://github.com/vanducng/rexboard
```

## Quick start

```bash
rexboard                 # the board, in this terminal
rexboard window          # the board in the Rex window you are in
rexboard ls --json       # every agent pane, most urgent first, as JSON
rexboard next            # go to the next pane that needs you
rexboard pick            # choose a pane from the list and go to it
```

The first command starts `rexboard serve` in the background. It watches this host and every host in `rex hosts`, and keeps what Rex does not: how long each pane has been in its state, and whether you have seen it.

## What "needs you" and "seen" mean

A pane needs you when it is blocked (often on a permission prompt), has an error, or is done and you have not seen it yet. Going to a pane marks it seen, and so does `s` on the board (`ctrl+s` marks every shown done row seen). A done pane you have seen moves to QUIET; a blocked or erroring one stays in NEEDS YOU until it moves on. A pane is unseen again each time it enters `done`, `blocked` or `error`.

## How it works

<img alt="rexboard architecture: local (this Mac) and mini each run one observer that feeds rexboard serve, which keeps state on disk and answers ls, watch, seen and the board over a unix socket; a jump, one of two writes to Rex, runs jump.pre_cmd if set, focuses the pane on its host and asks the Rex app to show its session" src="docs/assets/architecture.svg" width="100%">

rexboard reads Rex. Two commands write to it: a jump (blue) focuses the pane on its host and asks the Rex app to show it, and `rexboard window` makes the board's own session and shows it in the focused window.

## Docs

| Page | What it covers |
| --- | --- |
| [CLI](docs/cli.md) | Every command, its output, fields and exit codes |
| [TUI](docs/tui.md) | The board's layout and keys, and `pick` |
| [Configuration](docs/configuration.md) | `config.toml`: the app client, `jump.pre_cmd` (for a board in its own window) and excluded hosts |
| [System architecture](docs/system-architecture.md) | serve, the observers, the protocol and the files |
| [Development](docs/development-guidelines.md) | Setup, checks, layout, conventions, tests |
| [Deployment](docs/deployment.md) | CI, release-please, release assets, install, rollback |

## License

MIT. See [LICENSE](LICENSE).
