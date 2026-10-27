# 14. Writes to user config are explicit, reversible and fail open

**Status:** Accepted, 2026-10-27

## Context

[13](0013-rexboard-ships-the-hooks-it-relies-on.md) made rexboard write into files the user owns: Claude Code's and Codex's settings and pi's extensions. Those files run on every agent turn, and some are a dotfiles checkout. A hook that exits 2 blocks a Claude Code agent: it blocks tools, erases prompts and keeps a turn from stopping. A hook that names a missing binary fails on every event. Nothing a user can see says which.

## Decision

- **Explicit:** rexboard writes outside its own state only when a command asks it to (`hooks install`). Its preview (`--dry-run`, `hooks status`) runs the same code as the write, so what it shows is what gets written.
- **Merge, never replace:** every key and hook rexboard did not add stays, in its order. A file whose shape rexboard does not know is left alone.
- **Backup first:** a copy beside the file with the file's mode, never over an earlier backup, then the write by rename.
- **Never through a symlink:** a linked file belongs to the checkout it points into. rexboard reads it and says what it lacks, and the change is made there.
- **Reversible:** an uninstall removes exactly what install added and nothing else.
- **Fail open:** `rexboard hook` and `rexboard usage` exit 0 on every error of their own, bad arguments included; `usage` exits with the status line command's code when it runs one. Both say why on stderr and in `REXBOARD_HOOK_LOG`. The installed command names rexboard by its full path and wraps it in `|| true`, so a missing rexboard, or one too old to know `hook`, also exits 0. The status line goes through rexboard only while rexboard answers.

## Consequences

- A new write into a harness's files follows every bullet here, or comes with a record that says why it cannot.
- Until `hooks uninstall` ships, the undo is the backup or removing the entries by hand, as [CLI](../cli.md) says.
- Not built yet: `hooks status` saying what a linked file lacks (today it prints `symlink`).
- The fail-open rule costs the hook its error code: a broken install is found with `hooks status` and the hook log, not by the agent stopping.

## Supersedes

None. It adds the write rules to [13](0013-rexboard-ships-the-hooks-it-relies-on.md).
