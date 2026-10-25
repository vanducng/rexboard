# rexboard

An attention board for coding-agent panes across Rex hosts: which agent is working, blocked or done, for how long, and a jump to its pane.

Unofficial. Not affiliated with Superlogical or Rex.

This is an early scaffold. The binary only prints its version for now.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/vanducng/rexboard/main/install.sh | sh
```

This installs the latest release for macOS on Apple silicon or Linux x86_64 into `~/.local/bin`, after checking it against the release's `SHA256SUMS`.

Or build from source with Rust 1.88 or newer:

```bash
cargo install --git https://github.com/vanducng/rexboard
```

## Docs

| Page | What it covers |
| --- | --- |
| [Development](docs/development-guidelines.md) | Setup, checks, layout, conventions, tests |
| [Deployment](docs/deployment.md) | CI, release-please, release assets, install, rollback |

## License

MIT. See [LICENSE](LICENSE).
