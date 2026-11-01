# Configuration

rexboard reads `$XDG_CONFIG_HOME/rexboard/config.toml`, else `~/.config/rexboard/config.toml`. Every key is optional, and a missing file is an empty one.

```toml
# The Rex app client a jump selects the session in, by label or id.
client = "studio"

[hosts]
# `rex hosts` labels to leave out: no observer, no rows.
exclude = ["build-box"]

[extensions]
# Only these run, by name.
enabled = ["session-prefix"]
# Roots to look in after the config dir's `extensions`, each holding one directory per extension.
dirs = ["~/work/rexboard-extensions"]

[extensions.settings.session-prefix]
# Passed to that extension untouched.
separator = "-"

[models]
# A model name as a harness reports it, exactly, and what the board shows instead. Map the
# ids and display names the harnesses use to one style, here `name-version`; the value is shown
# as written.
"claude-opus-5-5" = "opus-5.5"
"Opus 5.5" = "opus-5.5"
```

| Key | Type | Default | What |
| --- | --- | --- | --- |
| `client` | string | none | The app client that takes jumps on this host, by its label (`rex client ls`) or id. `REX_CLIENT` in the environment wins over it. With neither, rexboard uses the only app connected over the unix socket, and fails with `AMBIGUOUS_CLIENT` when there is none there or more than one; the message says which and names each app. A jump to a remote host uses the app connected from this Mac's Tailscale node instead |
| `hosts.exclude` | array of strings | `[]` | Remote hosts to leave out of `ls`, `watch` and the board |
| `extensions.enabled` | array of strings | `[]` | The [extensions](extensions.md) serve runs. A directory dropped in runs nothing until it is named here |
| `extensions.dirs` | array of strings | `[]` | Roots to look in for extensions after `$XDG_CONFIG_HOME/rexboard/extensions`. `~/` is your home. The first extension of a name wins |
| `extensions.settings.NAME` | table | `{}` | Handed to extension `NAME` in every request, untouched |
| `models` | table of strings | `{}` | Model aliases. A model name a harness reports that equals a key, character for character (no prefix, no case folding), is shown as the value, as written, in the board's `MODEL` column and card, in `show`, and as `model_display` in `ls --json` and `show --json`, which keep the reported name in `model`; a name with no key is shown as reported. Harnesses name one model differently (pi reports `claude-opus-5-5`, Claude Code `Opus 5.5`), so this is where they are made to read alike. rexboard has no names of its own. A blank value is ignored; the value is held to 64 characters. It applies to the rows this host's serve shows, whichever host reported them |

A `[jump]` table is ignored, so an old config that still has one loads.

The board's own display settings (`icons`, `card_height`, `motion`, `columns`) live in `tui.json`, not here; see [TUI](tui.md#turn-card).

## When a change takes effect

serve reads the file when it starts and each time it rescans `rex hosts`, every 5 minutes. After `rexboard serve --stop`, the next command starts a serve that reads it at once. `jump`, `next`, `pick`, `ls --direct`, `watch --raw`, `watch --direct`, `ext ls` and `ext test` read it each time they run.

A file that cannot be read, or that has a key rexboard does not know, fails those commands with `CONFIG` (exit 1), naming the file and the key. serve keeps going: while the file is bad, a starting serve watches only this host, and a running one keeps the hosts it has.

## Which window a jump lands in

A jump asks the Rex app to select a session (`client.perform session.select`). That action takes the session and one of its tabs, but no app window, so the app switches whichever of its windows has focus. Checked on Rex Beta 0.1.0 (1408), server API revision 7:

- With two app windows, selecting a session switched the focused window every time and left the other one alone.
- When the session was already open in the other window, the app focused that window instead.
- No client action lists app windows or takes one as an argument.

So run the board in the Rex window you work in: a jump from it opens the agent's session in that same window.
