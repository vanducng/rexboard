# 4. Extensions, not a built-in core

**Status:** Accepted, 2026-10-25. Amended 2026-10-27

## Context

Tools around the agents (a seat manager, a project layout, a ticket system) each know something about a pane that Rex does not. Building each one into rexboard would tie the core to every tool. The operator asked for extensions, not a built-in core.

## Decision

- An extension is a directory: an `extension.toml` manifest and an adapter in any language. Each run, serve writes the rows as JSON on the adapter's stdin and reads fields per row from its stdout.
- The contract is versioned (`contract = 1`) and documented with samples a test parses ([Extensions](../extensions.md)).
- Only names listed in `extensions.enabled` run; dropping a directory in runs nothing.
- Contract 1 gives fields, and optionally a fallback status for rows with no record. It cannot act, add keys or add views. Fields become columns, views and filter keys on their own.
- Not in-process Lua or a plugin API: rexboard only fetches data, so a process with JSON in and out is enough, works in any language, and cannot crash or stall the board (timeouts, output caps, its own process group).
- No tool is named in `src/`.
- Harness adapters (`src/harness/`) are not extensions. They run inside the agent's hook on every event, and they read the dialogs answer mode's guard depends on ([7](0007-acting-on-panes-is-guarded.md)), so they stay compiled and pinned by captured fixtures. Extensions add data and never decide an action.

## Consequences

- A tool integration ships and versions on its own, and `ext test` checks it alone.
- An extension runs with your permissions; enabling it is a trust decision.
- Anything beyond fields and a fallback status needs a new contract version alongside 1.

## Supersedes

None. Amended 2026-10-27: the harness adapter bullet.
