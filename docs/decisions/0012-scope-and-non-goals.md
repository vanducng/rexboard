# 12. Scope and non-goals

**Status:** Accepted, 2026-10-25

## Context

An attention board can grow into a terminal manager, a notifier, a transcript reader and a plugin host. Saying what it is not keeps the work on direction.

## Decision

rexboard shows which agent needs you, for how long, and takes you there; it answers or closes a pane only through the guarded modes in [7](0007-acting-on-panes-is-guarded.md). These are out of scope:

- Writing Rex labels or layout: sessions, tabs and pane labels belong to Rex and the tools that set them.
- Fixing how a harness reports its status: that lives in the agent's setup ([3](0003-the-agent-tells-rex-rexboard-shows-it.md)).
- Reading transcripts, or scraping screens to detect state.
- A plugin API inside Rex, or sidebar badges: Rex documents neither.
- User-defined columns or layouts: extension fields are the only addition ([4](0004-extensions-not-a-built-in-core.md)).
- An extension registry, installer or sandbox.
- Windows.

Deferred, not refused: desktop notifications when a row enters NEEDS YOU, and history or stats views (the transitions are already kept in `transitions.jsonl`).

## Consequences

- A request on this list needs a new record that supersedes this one, not a quiet exception.

## Supersedes

None.
