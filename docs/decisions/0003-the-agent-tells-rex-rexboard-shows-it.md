# 3. The agent tells Rex, rexboard shows it

**Status:** Accepted, 2026-10-26. Its install bullet is superseded by [13](0013-rexboard-ships-the-hooks-it-relies-on.md)

## Context

A row is only useful if its state is right. Options were: agents report their status to Rex; rexboard scrapes each pane's screen; or rexboard reads each harness's transcript. Scraping every pane costs megabytes over the network at 40 agents and breaks on harness chrome. Transcripts need one integration per harness and do not reach remote hosts.

## Decision

- Status comes from the agent: it reports `state`, `kind` and `msg` to Rex as a status record (OSC 7501), natively or through a hook. rexboard installs nothing into agents; the hooks belong to the agent's setup.
- A pane with no record still shows, as `inferred` from its title glyph, under `UNREPORTED`, never mixed in with reported rows.
- A hook's multi-line message travels in `msg` joined with ` ⏎ `, and a `turn` sub-record carries a turn's final message where the agent's own `done` has none.
- Reading a pane's screen is a bounded, on-demand fallback: the card reads the last 48 KiB once per report for a blocked row, after its card has settled for 300 ms, and the full view (`space`) reads the last 96 KiB every 2 seconds only while it is open. Moving through rows reads nothing.

## Consequences

- Coverage is the agent setup's job; [Agent status](../agent-status.md) says how to check it.
- What rexboard shows is the same for local and remote rows, and costs nothing per view.
- An extension can add a fallback status for rows with no record ([4](0004-extensions-not-a-built-in-core.md)); a record always wins over it.

## Supersedes

The idea of capturing each pane's whole scrollback on selection.
