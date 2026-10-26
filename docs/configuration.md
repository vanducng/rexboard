# Configuration

rexboard reads `$XDG_CONFIG_HOME/rexboard/config.toml`, else `~/.config/rexboard/config.toml`. Every key is optional, and a missing file is an empty one. A file that cannot be read, or that has a key rexboard does not know, fails `jump`, `next`, `pick`, `ls --direct`, `watch --raw` and `watch --direct` with `CONFIG` (exit 1), naming the file and the key. serve reads the file when it starts and each time it rescans `rex hosts` (every 5 minutes); while the file is bad, a starting serve watches only this host, and a running one keeps the hosts it has.

```toml
# The Rex app client a jump selects the session in, by label or id.
client = "studio"

[hosts]
# `rex hosts` labels rexboard leaves out: no observer, no rows.
exclude = ["build-box"]
```

| Key | Type | Default | What |
| --- | --- | --- | --- |
| `client` | string | none | The app client that takes jumps on this host, by its label (`rex client ls`) or id. `REX_CLIENT` in the environment wins over it. Without either, rexboard uses the only app connected over the unix socket, and fails with `AMBIGUOUS_CLIENT` when no app is connected, when none is on the unix socket, or when more than one is; the message says which, and the next steps name each app. Jumps to a remote host use the app connected from this Mac's Tailscale node instead |
| `hosts.exclude` | array of strings | `[]` | Remote hosts to leave out of `ls`, `watch` and the board. After a change, run `rexboard serve --stop` so serve rescans |

An older config may still have a `[jump]` table with `pre_cmd`, for a board kept in its own Rex window. rexboard no longer runs it and ignores the table, so such a file still loads.

## Which window a jump lands in

A jump asks the Rex app to select a session (`client.perform session.select`). The action takes the session and one of its tabs, but no app window: the app switches whichever of its windows has focus. Checked on Rex Beta 0.1.0 (1408), server API revision 7:

- With two app windows, selecting a session switched the focused window every time, and left the other one as it was.
- When the session was already open in the other window, the app focused that window instead of switching any.
- No client action lists app windows or takes one as an argument.

So run the board in the Rex window you work in: a jump from it opens the agent's session in that same window.
