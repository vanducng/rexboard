# Configuration

rexboard reads `$XDG_CONFIG_HOME/rexboard/config.toml`, else `~/.config/rexboard/config.toml`. Every key is optional, and a missing file is an empty one. A file that cannot be read, or that has a key rexboard does not know, fails `jump`, `next`, `pick`, `ls --direct`, `watch --raw` and `watch --direct` with `CONFIG` (exit 1), naming the file and the key. serve reads the file when it starts and each time it rescans `rex hosts` (every 5 minutes); while the file is bad, a starting serve watches only this host, and a running one keeps the hosts it has.

```toml
# The Rex app client a jump selects the session in, by label or id.
client = "studio"

[hosts]
# `rex hosts` labels rexboard leaves out: no observer, no rows.
exclude = ["build-box"]

[extensions]
# Only these run, by name. A directory dropped in runs nothing until it is named here.
enabled = ["session-prefix"]
# Roots to look in after the config dir's `extensions`, each holding one directory per extension.
dirs = ["~/work/rexboard-extensions"]

[extensions.settings.session-prefix]
# Passed to that extension untouched.
separator = "-"
```

| Key | Type | Default | What |
| --- | --- | --- | --- |
| `client` | string | none | The app client that takes jumps on this host, by its label (`rex client ls`) or id. `REX_CLIENT` in the environment wins over it. Without either, rexboard uses the only app connected over the unix socket, and fails with `AMBIGUOUS_CLIENT` when no app is connected, when none is on the unix socket, or when more than one is; the message says which, and the next steps name each app. Jumps to a remote host use the app connected from this Mac's Tailscale node instead |
| `hosts.exclude` | array of strings | `[]` | Remote hosts to leave out of `ls`, `watch` and the board. After a change, run `rexboard serve --stop` so serve rescans |
| `extensions.enabled` | array of strings | `[]` | The extensions serve runs. An extension is a directory with an `extension.toml` and an adapter; see [Extensions](#extensions) |
| `extensions.dirs` | array of strings | `[]` | Roots to look in for extensions after `$XDG_CONFIG_HOME/rexboard/extensions`. `~/` is your home. The first extension of a name wins |
| `extensions.settings.NAME` | table | `{}` | Handed to extension `NAME` in every request, untouched |

## Extensions

An extension adds fields to rows, and may give a row with no status of its own a fallback status, from any tool, without rexboard knowing the tool. It is a directory holding `extension.toml` and an adapter command:

```toml
name = "session-prefix"  # [a-z0-9-]+, the directory's name
contract = 1             # the contract the adapter speaks
cmd = ["./adapter"]      # argv, no shell; a leading ./ is the extension's directory
every = 10               # seconds between runs, at least 2
timeout = 5              # seconds, less than every; a slower run is killed
[[fields]]
name = "project"         # [a-z0-9-]+, stored on a row as session-prefix.project
title = "PROJECT"
[provides]
status = false           # true lets it give rows with no record a status
```

serve runs each enabled extension on its own thread: every `every` seconds it writes the current rows to the adapter's stdin as JSON and reads fields per row back from its stdout. A row's own status record always wins over an extension's status. A failing run (an error exit, bad JSON, another contract or a timeout) keeps the last good answer for 3 runs, then drops it. Problems go to `serve.log`, as `ext NAME: …`, along with the first 4 KiB of the adapter's stderr per run. Anything the adapter leaves running, such as a process started with `&`, is stopped when it exits. `ls --json` shows each row's fields under `ext`; an extension that stops running takes its fields and status off the rows.

An extension runs with your permissions and sees every row's labels and titles, so enable only adapters you trust. serve reads the manifests and `[extensions]` when it starts and each time it rescans; `rexboard serve --stop` picks up a change at once.

An older config may still have a `[jump]` table with `pre_cmd`, for a board kept in its own Rex window. rexboard no longer runs it and ignores the table, so such a file still loads.

## Which window a jump lands in

A jump asks the Rex app to select a session (`client.perform session.select`). The action takes the session and one of its tabs, but no app window: the app switches whichever of its windows has focus. Checked on Rex Beta 0.1.0 (1408), server API revision 7:

- With two app windows, selecting a session switched the focused window every time, and left the other one as it was.
- When the session was already open in the other window, the app focused that window instead of switching any.
- No client action lists app windows or takes one as an argument.

So run the board in the Rex window you work in: a jump from it opens the agent's session in that same window.
