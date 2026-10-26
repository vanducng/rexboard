# Development guidelines

Setup, the checks, conventions and pull request rules are in [CONTRIBUTING](../CONTRIBUTING.md). This page maps the code and the tests.

## Layout

- `src/main.rs` parses and reports errors. `src/lib.rs` dispatches.
- `src/cli.rs` is the clap surface and the agent contract. Help text changes are interface changes.
- `src/error.rs` is the error type: a code, a message, and `next_steps`.

[System architecture](system-architecture.md#pieces) maps the rest.

## Tests

| Kind | Where | Runs |
| --- | --- | --- |
| Unit | `#[cfg(test)]` modules next to the code | Pure logic |
| CLI | `tests/cli.rs` | The real binary against `tests/fixtures/fake-rex`, its output, and exit codes |

Three tests read files you may edit:

- `every_screen_matches_the_style_snapshot` renders the board's screens cell by cell against `tests/fixtures/style-snapshot.txt`. After a deliberate change of look, record it again with `UPDATE_STYLE_SNAPSHOT=1 cargo test every_screen` and read the diff.
- The extension contract test parses the `json request` and `json response` blocks in [Extensions](extensions.md), so keep them valid.
- Answer mode's screen tests read recorded, scrubbed Claude Code screens from `tests/fixtures/screens/`.

`scripts/record-demo.sh` records the README demo on a private, throwaway Rex server, and refuses to run against yours.
