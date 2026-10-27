# 5. The CLI is the API

**Status:** Accepted, 2026-10-25. Amended 2026-10-27

## Context

Agents and scripts use rexboard as much as people do. They need stable output and errors they can act on, not a screen to parse.

## Decision

- The rows and what serve remembers about them, what the board's card shows, marking seen, jumping, answering and closing a pane, and extensions are all reachable from the CLI (`ls`, `watch`, `seen`, `show`, `views`, `hosts`, `jump`, `next`, `pick`, `answer`, `close`, `ext`, `upgrade`), and every command takes `--json`.
- `answer` and `close` keep the board's guards, as [7](0007-acting-on-panes-is-guarded.md) says.
- Errors carry a code, a message that names the bad value, and `next_steps` a caller can run: `{"ok":false,"error":{"code","message","next_steps"}}` on stderr with `--json`. Exit 1 is a failure; exit 2 is bad usage, or every host unreachable.
- `rexboard --help` and [CLI](../cli.md) are the contract. Adding a field, a code or a flag is fine. Renaming or removing one is a breaking change. Key order is not part of the contract: parse the JSON, do not match its text.
- The observer line contract and serve's protocol carry `"v": 1`.

## Consequences

- A new board feature that reads data comes with its CLI path, or says why not.
- Help text changes are interface changes and go through review like code.
- Breaking changes are rare and deliberate. Below 1.0 one is a minor version, as [9](0009-release-and-install.md) says; from 1.0, a major.

## Supersedes

None. Amended 2026-10-27: key order, and versioning in line with [9](0009-release-and-install.md).
