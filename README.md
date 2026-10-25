# rexboard

An attention board for coding-agent panes across Rex hosts: which agent is working, blocked or done, for how long, and a jump to its pane.

Unofficial. Not affiliated with Superlogical or Rex.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/architecture-dark.svg">
  <img alt="rexboard architecture: one observer per Rex host feeds rexboard serve, which keeps state on disk and answers ls, watch, seen and the board over a unix socket; a jump runs jump.pre_cmd, focuses the pane on its host and asks the Rex app to select its session" src="docs/assets/architecture.svg" width="100%">
</picture>

How it fits together: [system architecture](docs/system-architecture.md).

This is an early scaffold. The binary only prints its help and version for now; the board arrives in later releases.

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
