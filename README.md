# rexboard

An attention board for coding-agent panes across Rex hosts: which agent is working, blocked or done, for how long, and a jump to its pane.

Unofficial. Not affiliated with Superlogical or Rex.

<img alt="rexboard architecture: local (this Mac) and mini each run one observer that feeds rexboard serve, which keeps state on disk and answers ls, watch, seen and the board over a unix socket; a jump, the one write to Rex, runs jump.pre_cmd, focuses the pane on its host and asks the Rex app to show its session" src="docs/assets/architecture.svg" width="100%">

rexboard only reads Rex. The one write is a jump (blue): it focuses the pane on its host and asks the Rex app to show it.

How it fits together: [system architecture](docs/system-architecture.md).

Run `rexboard` in a terminal to open the board; `rexboard --help` lists the commands.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
```

This installs the latest release for macOS on Apple silicon or Linux x86_64 into `~/.local/bin`, after checking it against the release's `SHA256SUMS`.

Or build from source with Rust 1.88 or newer:

```bash
cargo install --locked --git https://github.com/vanducng/rexboard
```

## Docs

| Page | What it covers |
| --- | --- |
| [Development](docs/development-guidelines.md) | Setup, checks, layout, conventions, tests |
| [Deployment](docs/deployment.md) | CI, release-please, release assets, install, rollback |

## License

MIT. See [LICENSE](LICENSE).
