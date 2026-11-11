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

# Rows to mark. Every key of one rule must match; any rule marks the row.
[[mark]]
session = "api"
block = "seat-*"

[[mark]]
host = "studio"
field."session-prefix.project" = "api-?"
```

| Key | Type | Default | What |
| --- | --- | --- | --- |
| `client` | string | none | The app client that takes jumps on this host, by its label (`rex client ls`) or id. `REX_CLIENT` in the environment wins over it. With neither, rexboard uses the only app connected over the unix socket, and fails with `AMBIGUOUS_CLIENT` when there is none there or more than one; the message says which and names each app. A jump to a remote host uses the app connected from this Mac's Tailscale node instead |
| `hosts.exclude` | array of strings | `[]` | Remote hosts to leave out of `ls`, `watch` and the board |
| `extensions.enabled` | array of strings | `[]` | The [extensions](extensions.md) serve runs. A directory dropped in runs nothing until it is named here |
| `extensions.dirs` | array of strings | `[]` | Roots to look in for extensions after `$XDG_CONFIG_HOME/rexboard/extensions`. `~/` is your home. The first extension of a name wins |
| `extensions.settings.NAME` | table | `{}` | Handed to extension `NAME` in every request, untouched |
| `models` | table of strings | `{}` | Model aliases. A model name a harness reports that equals a key, character for character (no prefix, no case folding), is shown as the value, as written, in the board's `MODEL` column and card, in `show`, and as `model_display` in `ls --json` and `show --json`, which keep the reported name in `model`; a name with no key is shown as reported. Harnesses name one model differently (pi reports `claude-opus-5-5`, Claude Code `Opus 5.5`), so this is where they are made to read alike. rexboard has no names of its own. A blank value is ignored; the value is held to 64 characters. It applies to the rows this host's serve shows, whichever host reported them |
| `mark` | array of tables | `[]` | `[[mark]]` rules: the rows to [mark](tui.md#marked-rows). See below |
| `hooks.ask.pattern` | string | none | Case-sensitive prefix of the first non-blank line of the last assistant message at `Stop`; no built-in pattern |
| `hooks.ask.reply` | string | none | A line appended to the question in the reported message |
| `hooks.ask.label` | string | `decision` | The `ask` label on a matching row |

A `[jump]` table is ignored, so an old config that still has one loads.

## `[[mark]]`

Each `[[mark]]` table is one rule, and a row is marked when any rule matches it. A rule holds one or more of these keys, and every key it has must match:

| Key | Matches |
| --- | --- |
| `session` | the session label |
| `window` | the window (tab) label |
| `block` | the block (pane) label |
| `host` | `local`, or the label from `rex hosts` |
| `field."<id>"` | an extension field, by its id as `ext` in [`ls --json`](cli.md#json-fields) names it (`<extension>.<field>`). The id has a dot, so it is quoted; `"field.<id>" = ...` is the same key. A row without that field does not match |

A value is an exact label, or a glob: `*` stands for any run of characters, none too, and `?` for any one character. The whole label must match, and case counts. A rule that has no keys, a key not listed here, or a value that is not text is skipped, and named once in `serve.log` (`ls --direct` prints it on stderr); the other rules still apply. A `mark` that is not an array of tables is skipped the same way. The file still loads, so a bad rule never takes a mark away from a good one.

serve applies the rules, so the board, `pick` and `ls` show the same marks: an [extension](extensions.md#the-response) can mark rows too, and serve unions both. Your own `*` on the board marks rows too ([favorites](tui.md#favorites), kept in `favorites.json`, not in this file). `ls --json` gives each row `marked` and `marked_by` (`["config"]`, with an extension `["config", "team"]`, and `favorite` last), and `ls --marked` lists only the marked rows (`--marked SOURCE` one source's, see [`ls`](cli.md#rexboard-ls)). `ls --direct` applies them too, but runs no extension, so it sees no `field` match. A broad glob can mark most of the board; `rexboard ls --marked` shows what the rules catch. The board draws a mark as `*` before the row's number, see [Marked rows](tui.md#marked-rows). `'` on the board and in `pick` goes to a marked row, and `rexboard next --marked` does the same from a script. [Decision 18](decisions/0018-rows-can-be-marked.md) records the design.

The board's own display settings (`icons`, `card_height`, `motion`, `columns`, `digits`) live in `tui.json`, not here; see [TUI](tui.md#turn-card) and, for `digits`, [Digits](tui.md#digits).

## `[hooks.ask]`

```toml
[hooks.ask]
pattern = "**ASK"
reply = "Reply here with your choice"
label = "decision"
```

At `Stop`, [`rexboard hook`](cli.md#rexboard-hook) checks the first non-blank line of the last assistant message. If it starts with `pattern`, the pane reports `blocked` with kind `question`. Its message is that line, plus `reply` when set, joined with ` ⏎ `. The `ask` field carries `label`, defaulting to `decision`. The card shows `ask decision`; `/ask:decision` separates it from an on-screen dialog. It stays in NEEDS YOU after a jump until the next prompt clears it.

The rule applies where the hook owns the pane's status: inside tmux, including agent and human seats. In plain Rex panes, pi 1.5.1, Claude Code and Codex report natively, so a finished turn stays `done` for now. pi's native `done` lands after its hook; any older `ask` is ignored, leaving `ask: null`. The hook does not delay or retry to override it. The hook reads this host's config on each `Stop`, so no agent restart is needed for a rule change. No match, no rule, an unreadable or invalid config, or a blank pattern or label leaves today's `done` behavior. A bad config adds one error line to `REXBOARD_HOOK_LOG`, without its contents. The rule runs no command and reads no screen or transcript. Upgrade serve's host first ([Agent status](agent-status.md#decision-signals)).

## When a change takes effect

serve reads the file when it starts and each time it rescans `rex hosts`, every 5 minutes; a new or changed `[[mark]]` rule marks rows at that read. After `rexboard serve --stop`, the next command starts a serve that reads it at once. `jump`, `next`, `pick`, `ls --direct`, `watch --raw`, `watch --direct`, `ext ls` and `ext test` read it each time they run.

A file that cannot be read, or that has a key rexboard does not know, fails those commands with `CONFIG` (exit 1), naming the file and the key. serve keeps going and loses nothing: while the file is bad, the host scan runs with no `[hosts] exclude` in a serve that has not read a good file yet, and with the last good one in a serve that has. Models and `[[mark]]` rules stay as the last good read left them (none, in a fresh serve). serve says so: the board's top bar shows `config.toml bad: line 12` in red, `ls` prints one `warning:` on stderr (exit 0), and `serve.log` has it once. Fixing the file clears all three at serve's next read, as above.

## Which window a jump lands in

A jump asks the Rex app to select a session (`client.perform session.select`). That action takes the session and one of its tabs, but no app window, so the app switches whichever of its windows has focus. Checked on Rex Beta 0.1.0 (1408), server API revision 7:

- With two app windows, selecting a session switched the focused window every time and left the other one alone.
- When the session was already open in the other window, the app focused that window instead.
- No client action lists app windows or takes one as an argument.

So run the board in the Rex window you work in: a jump from it opens the agent's session in that same window.
