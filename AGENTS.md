# rexboard

Unofficial attention board for agent panes across Rex hosts. The binary is `rexboard`.

`--help` is the command contract. Do not treat the Rust library API as stable.

Follow [CONTRIBUTING.md](CONTRIBUTING.md) and the records in [docs/decisions](docs/decisions/README.md); a change that goes against a decision proposes a new record.

## Layout

- `src/lib.rs` dispatches commands. `src/main.rs` only parses and reports errors.
- `src/cli.rs` is the clap surface. `src/error.rs` holds the error type and prints it, as JSON with `--json`.
- `install.sh` installs a release.
- `scripts/record-demo.sh` records the README demo (`docs/assets/demo.cast` and `demo.gif`) on a private, throwaway Rex server. Re-record with it rather than by hand, so the cast never sees real panes.
- `tests/cli.rs` runs the binary.

## Checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

User docs live in `docs/`: cli, tui, agent status, configuration, extensions, system architecture, development, deployment and decisions. Keep `README.md` a short hub.

## Privacy

This repo is public. Use generic names only in code, tests, fixtures, docs, screenshots and commit messages: hosts like `studio` and `mini`, sessions like `api` and `web`. Never commit tailnet hostnames, real session, tab or pane labels, home paths, or transcripts. Scrub recorded fixtures before committing. The demo script refuses a cast with home or temp paths, and with any string from the file `REXBOARD_DEMO_DENY` names; give it your real labels.

## Releases

Do not commit `target/` or tokens. The crate is not published to crates.io (`publish = false`). Releases come from release-please: merge its release PR to bump the version and tag `vX.Y.Z`; `.github/workflows/release.yml` then builds and attaches the archives. Do not edit `CHANGELOG.md` or bump `Cargo.toml` by hand.
