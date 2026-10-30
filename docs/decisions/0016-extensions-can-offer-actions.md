# 16. Extensions can offer actions

**Status:** Accepted, 2026-10-31

## Context

[4](0004-extensions-not-a-built-in-core.md) kept extensions to data: fields and a fallback status. A tool around the agents can also do something with a row, such as send a message to the seat in it. Building that in would name the tool in `src/`. Letting an adapter act on its own would put a write behind every refresh.

## Decision

- An extension may declare `[[actions]]`: a name, title, one-character key, an argv template `cmd` that holds `{text}`, and optionally `confirm` (the target, in the manifest's words), `when`, `max` and `timeout`. An action sends one line the user types. It declares; rexboard owns the prompt, the checks and the run.
- Placeholders are `{host}`, `{field:NAME}` and `{text}`, each expanded into one argv element. There is no shell, so the text cannot add or change an argument, and the program cannot be a placeholder.
- A field may be `hidden`: it reaches the row (`ls --json`, placeholders, `when`) with no column, view, filter key or card fact.
- The board starts one with `x` and the action's key, as guarded as [7](0007-acting-on-panes-is-guarded.md): an input line for the text, then a red confirm line that names the target and always shows the typed text in quotes, whatever the manifest's `confirm` says, then `y`, only once the line has been up for 300 ms. Any other key cancels.
- The text is one line with no control characters, at most `max` bytes (4096 at the most). Nothing runs before `y`.
- At `y` the whole plan (argv, confirm, environment) is built again from the extension's current offer and the row as it stands. If it differs from what was shown, or the action or the row is gone, nothing runs.
- A run has a timeout (at most 60 s) and its own process group, killed on timeout and when the board exits. It has no stdin, a 4 KiB cap on what is kept of each output, and `REXBOARD_EXTENSION`, `REXBOARD_ACTION` and `REXBOARD_SETTINGS` in its environment. A failure shows its exit and the first line it said, in the alert role.
- The board runs the action in its own process, not in serve, so nothing outlives it.
- `rexboard ext run NAME ACTION KEY` does the same from the command line, with `--text`, `--dry-run`, and `--yes` (or `y` on a terminal), and never runs without one of them.

## Consequences

- An action runs with your permissions, like an adapter. Enabling an extension is the trust decision.
- A key two extensions both declare goes to the first enabled; the other is left out and logged once.
- The contract stays at 1: `actions` and `hidden` are manifest keys. A rexboard without them marks such a manifest broken, as it does for any key it does not know.

## Supersedes

Amends [4](0004-extensions-not-a-built-in-core.md): contract 1 can no longer be said to be unable to act.
