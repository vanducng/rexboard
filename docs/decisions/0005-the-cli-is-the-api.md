# 5. The CLI is the API

**Status:** Accepted, 2026-10-25

## Context

Agents and scripts use rexboard as much as people do. They need stable output and errors they can act on, not a screen to parse.

## Decision

- The rows and what serve remembers about them, what the board's card shows, marking seen, jumping, answering and closing a pane, and extensions are all reachable from the CLI (`ls`, `watch`, `seen`, `show`, `views`, `hosts`, `jump`, `next`, `pick`, `answer`, `close`, `ext`, `upgrade`), and every command takes `--json`.
- `answer` and `close` keep the board's guards ([7](0007-acting-on-panes-is-guarded.md)): the same fresh read and the same one-`rex do` re-check before a send, `--confirm` where the board takes a second press, and `--yes` (or a `y` on a terminal) where `dd` asks. A failed check is `REFUSED` and sends nothing.
- Errors carry a code, a message that names the bad value, and `next_steps` a caller can run: `{"ok":false,"error":{"code","message","next_steps"}}` on stderr with `--json`. Exit 1 is a failure; exit 2 is bad usage, or every host unreachable.
- `rexboard --help` and [CLI](../cli.md) are the contract. Adding a field, a code or a flag is fine. Renaming or removing one is a breaking change.
- The observer line contract and serve's protocol carry `"v": 1`.

## Consequences

- A new board feature that reads data comes with its CLI path, or says why not.
- Help text changes are interface changes and go through review like code.
- Breaking changes need a major version, so they are rare and deliberate.

## Supersedes

None.
