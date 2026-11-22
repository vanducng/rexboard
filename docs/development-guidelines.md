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
| Hooks | `tests/hooks.rs` | `rexboard hook` and `rexboard hooks`, run as an agent runs them: a fake `tmux`, and a fake home with each harness's settings |
| Lua | `tests/lua.rs` | `observer.lua` and `send.lua` under a Lua interpreter, against a stand-in for Rex's API |
| Direction | `tests/direction.rs` | The docs: every JSON sample parses, `docs/cli.md` names every command and flag, no private path or host, the decisions index is whole |

Three tests read files you may edit:

- `every_screen_matches_the_style_snapshot` renders the board's screens cell by cell against `tests/fixtures/style-snapshot.txt`. After a deliberate change of look, record it again with `UPDATE_STYLE_SNAPSHOT=1 cargo test every_screen` and read the diff.
- The extension contract test parses the `json request` and `json response` blocks in [Extensions](extensions.md), so keep them valid.
- Answer mode's screen tests read recorded, scrubbed Claude Code screens from `tests/fixtures/screens/`.

`scripts/record-demo.sh` records the README demo on three private, throwaway Rex servers (this host, `mini` and `studio`, all unix sockets under a temp dir), and refuses to run against yours. The README has only this one recording, so it stays true as the board changes; re-record it when the board looks different.
