# Development guidelines

## Setup

Rust 1.88 or newer. The lockfile is committed.

```bash
git clone https://github.com/vanducng/rexboard
cd rexboard
cargo build
cargo run -- --help
```

To install from source instead of a release:

```bash
cargo install --locked --git https://github.com/vanducng/rexboard
```

## Checks

CI runs these, and a change is not done until they pass:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Layout

- `src/main.rs` parses and reports errors. `src/lib.rs` dispatches.
- `src/cli.rs` is the clap surface and the agent contract. Help text changes are interface changes.
- `src/error.rs` is the error type: a code, a message, and `next_steps`.

[System architecture](system-architecture.md#pieces) maps the rest.

## Conventions

- Default to no comments. Write one only when a reader would be surprised: a Rex quirk, a hidden constraint, a non-obvious invariant.
- CLI output is a contract. New fields are fine. Renaming or removing a JSON field, an error code, or a flag is a breaking change.
- Errors carry a code, a message that names the bad value, and `next_steps` an agent can run. With `--json` they print as `{"ok":false,"error":{code,message,next_steps}}` on stderr. Usage errors exit 2.
- Use generic names in tests, fixtures, docs, and screenshots: hosts like `studio` and `mini`, sessions like `api` and `web`. Never commit tailnet hostnames, real labels, home paths, or transcripts.
- Commits follow Conventional Commits, because release-please reads them. See [Deployment](deployment.md#releases).

## Tests

| Kind | Where | Runs |
| --- | --- | --- |
| Unit | `#[cfg(test)]` modules next to the code | Pure logic |
| CLI | `tests/cli.rs` | The real binary against `tests/fixtures/fake-rex`, its output, and exit codes |

Three tests read files you may edit:

- `every_screen_matches_the_style_snapshot` renders the board's screens cell by cell against `tests/fixtures/style-snapshot.txt`. After a deliberate change of look, record it again with `UPDATE_STYLE_SNAPSHOT=1 cargo test every_screen` and read the diff.
- The extension contract test parses the `json request` and `json response` blocks in [Extensions](extensions.md), so keep them valid.
- Answer mode's screen tests read real Claude Code screens from `tests/fixtures/screens/`.

`scripts/record-demo.sh` records the README demo on a private, throwaway Rex server, and refuses to run against yours.
