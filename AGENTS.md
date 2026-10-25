# rexboard

Unofficial attention board for agent panes across Rex hosts. The binary is `rexboard`.

`--help` is the command contract. Do not treat the Rust library API as stable.

## Layout

- `src/lib.rs` dispatches commands. `src/main.rs` only parses and reports errors.
- `src/cli.rs` is the clap surface. `src/error.rs` holds the error type and prints it, as JSON with `--json`.
- `install.sh` installs a release.
- `tests/cli.rs` runs the binary.

## Checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

User docs live in `docs/`: development and deployment. Keep `README.md` a short hub.

## Privacy

This repo is public. Use generic names only in code, tests, fixtures, docs, screenshots and commit messages: hosts like `studio` and `mini`, sessions like `api` and `web`. Never commit tailnet hostnames, real session, tab or pane labels, home paths, or transcripts. Scrub recorded fixtures before committing.

## Releases

Do not commit `target/` or tokens. `publish = false`. Releases come from release-please: merge its release PR to bump the version and tag `vX.Y.Z`; `.github/workflows/release.yml` then builds and attaches the archives. Do not edit `CHANGELOG.md` or bump `Cargo.toml` by hand.
