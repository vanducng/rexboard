# 15. Scope and non-goals

**Status:** Accepted, 2026-10-27

## Context

[12](0012-scope-and-non-goals.md) kept harness setup out of scope. Since then rexboard ships the hooks agents report through ([13](0013-rexboard-ships-the-hooks-it-relies-on.md), [14](0014-writes-to-user-config-are-explicit-reversible-and-fail-open.md)) and shows each pane's model and context from those hooks. The scope needs to say so, and keep every line it did not change.

## Decision

rexboard shows which agent needs you, for how long, and takes you there; it answers or closes a pane only through the guarded modes in [7](0007-acting-on-panes-is-guarded.md). It ships the hooks that make an agent report, and shows the usage figures those hooks and status lines hand it.

These are out of scope:

- Writing Rex labels or layout: sessions, tabs and pane labels belong to Rex and the tools that set them.
- Reading transcripts, or scraping screens to detect state.
- A plugin API inside Rex, or sidebar badges: Rex documents neither.
- In-process plugins: an extension is a process with JSON in and out ([4](0004-extensions-not-a-built-in-core.md)).
- Themes, and user-defined columns, views or layouts: extension fields are the only addition.
- An extension registry, installer or sandbox, and installing anything on its own.
- Editing a harness config rexboard has no merge for.
- History or charts from usage.
- Windows.

Deferred, not refused: desktop notifications when a row enters NEEDS YOU, and history or stats views of state (the transitions are already kept in `transitions.jsonl`).

## Consequences

- A request on this list needs a new record that supersedes this one, not a quiet exception.
- A harness joins the hooks only when rexboard can merge into its config; until then its docs say what to add by hand, as grok's do.

## Supersedes

[12](0012-scope-and-non-goals.md): harness reporting is now in scope as the hooks rexboard ships. Every non-goal of 12 carries forward.
