# 13. rexboard ships the hooks it relies on

**Status:** Accepted, 2026-10-27. Amended 2026-10-27, 2026-10-31

## Context

[3](0003-the-agent-tells-rex-rexboard-shows-it.md) left the hooks to each agent's setup. In practice that setup was one person's dotfiles: a Python bridge that maps hook events to Rex statuses, a script that merges it into each harness's settings, and a pi extension. Inside tmux a row was only right on a machine with those dotfiles, and the per-turn usage (MODEL, CTX) needs the same hooks and status lines. The operator asked for harness support that works on a machine without OpenRig or the dotfiles.

## Decision

- `rexboard hook HARNESS [EVENT]` is the bridge, ported byte for byte from the Python one: the same statuses, the same OSC 7501 sequences, the same tmux passthrough. A corpus recorded from the Python bridge checks it.
- `rexboard hooks install` puts `rexboard hook` and `rexboard usage` into Claude Code's and Codex's settings and adds a pi extension. It never runs on its own. It merges: every other key and hook stays, a backup comes first, and a second run changes nothing. A settings file that is a symlink belongs to a dotfiles checkout and is left alone. `rexboard hooks status` says what an install would change.
- The status still comes from the agent. rexboard writes only the hooks that make an agent report, and only when asked.
- Not ported: the title spinner that puts Claude Code's glyph back on a plain Rex pane's title is a dotfiles nicety, not reporting, and stays there. grok's status line lives in `~/.grok/config.toml`, which rexboard does not edit.

## Consequences

- A fresh machine needs `rexboard hooks install` once; `rexboard hooks status` shows what it would do.
- How rexboard writes those files, and how a hook fails open, is [14](0014-writes-to-user-config-are-explicit-reversible-and-fail-open.md).
- `hooks status` reads a symlinked settings file too and says what it lacks; the change is made in the file it links to. A rexboard hook counts as present by any path that would run on this host, and in a portable form a file shared by several hosts can hold, so install leaves a working hook as it is and writes only what is missing. Install itself writes this host's own path, not the portable form: it finds rexboard under any install method, where the portable form knows only the two places `install.sh` and Homebrew use.
- The hook vocabulary lives in one place, tested against the bridge it replaces. A harness change to its hook JSON is a rexboard release.
- `serde_json`'s `preserve_order` feature (approved 2026-10-26) keeps a merged settings file in its own key order. Every JSON object rexboard prints now keeps the order it is built in, not sorted keys.

## Supersedes

The "rexboard installs nothing into agents" bullet of [3](0003-the-agent-tells-rex-rexboard-shows-it.md). Amended 2026-10-27: the link to 14, and status on a symlinked file. Amended 2026-10-31: any path, and the portable form.
