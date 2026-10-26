# 1. Reach Rex only through the `rex` CLI

**Status:** Accepted, 2026-10-25

## Context

rexboard needs every pane's status, title and process on every Rex host, and a few writes: focus a pane, answer it, close it. Rex is in beta, and its socket protocol, files and screens can change under us. Remote hosts need Rex's own transport and auth.

## Decision

- rexboard talks to Rex only through the `rex` command: `rex [-S HOST] do -` to run Lua scripts built into the binary, `rex hosts --json` for the hosts, and `rex block close` for a close.
- It never opens Rex's socket, reads Rex's private files, or scrapes Rex's own UI. Reading a pane's text goes through Rex's API (`format` and `blob_read`), bounded.
- Each host runs one long-lived observer script; one-shot scripts handle snapshots, jumps, tails and answers.
- The scripts live in `src/lua/` and ship inside the binary, so the binary and the scripts never disagree.

## Consequences

- Remote hosts work the same as this one, with no extra auth or ports.
- Each one-shot costs a process spawn (tens of milliseconds). That is fine for actions; observers are long-lived.
- Rex event shapes may change. Fixtures pin the observer contract (`v` 1), and errors name the Rex version.
- A feature that needs something the API does not expose waits for Rex, rather than reaching around it.

## Supersedes

None.
