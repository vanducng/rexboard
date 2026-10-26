# 2. One serve daemon, many clients

**Status:** Accepted, 2026-10-25

## Context

The board, `ls`, `watch`, `next`, `jump` and `pick` all need the same rows, and the same memory Rex does not keep: how long each pane has been in its state, and whether you have seen it. One observer per host per client would multiply Tailscale connections and disagree on `seen`.

## Decision

- `rexboard serve` owns one observer per host and the state (`state.json`, `transitions.jsonl`), and answers every client over one unix socket in the state dir.
- Clients start serve when nothing answers. Nobody has to run it by hand.
- Every client asks serve its version first and replaces a serve older than itself, once, so an upgrade takes effect on the next command.
- serve uses std threads and channels, not an async runtime: a few child processes and one socket do not need one. Only the reducer thread touches the board.

## Consequences

- Every board and every command see one `seen` and one time in state, and those survive a restart.
- serve keeps the environment of the command that started it; changing `REXBOARD_REX` needs `serve --stop`.
- `ls --direct`, `watch --raw` and `watch --direct` stay as serve-free paths for debugging Rex itself.
- The socket path must stay under 103 bytes, which limits how deep `XDG_STATE_HOME` can be.

## Supersedes

None.
