# Decisions

Each record says what rexboard decided, why, and what it costs. They are short on purpose: the docs say how things work, and these say why they work that way. Change one of these directions only by proposing a new record.

## Principles

These decide between options the records do not cover.

1. **Earn each mechanism.** A new mechanism, dependency, file or setting needs a record naming what it replaces, or why nothing existing could.
2. **The CLI is the API; the board is a view.** Presentation (icons, markdown, colour) never enters `--json`, `show`, or what `yy` copies.
3. **One definition per thing.** Where a user can define something, the built-ins use the same format.
4. **Words by default.** Everything reads without colour or a Nerd Font; icons and markdown add, never replace.
5. **Writes outside rexboard's state follow [14](0014-writes-to-user-config-are-explicit-reversible-and-fail-open.md);** a hook never exits 2.

## Records

| # | Decision | Status |
| --- | --- | --- |
| [1](0001-reach-rex-only-through-the-rex-cli.md) | Reach Rex only through the `rex` CLI | Accepted; amended |
| [2](0002-one-serve-daemon-many-clients.md) | One serve daemon, many clients | Accepted |
| [3](0003-the-agent-tells-rex-rexboard-shows-it.md) | The agent tells Rex, rexboard shows it | Accepted; install bullet superseded by 13; amended |
| [4](0004-extensions-not-a-built-in-core.md) | Extensions, not a built-in core | Accepted; amended |
| [5](0005-the-cli-is-the-api.md) | The CLI is the API | Accepted; amended |
| [6](0006-vim-keymap-from-one-table.md) | A vim keymap from one table | Accepted |
| [7](0007-acting-on-panes-is-guarded.md) | Acting on panes is guarded | Accepted |
| [8](0008-one-style-system.md) | One style system | Accepted; amended |
| [9](0009-release-and-install.md) | Release and install | Accepted |
| [10](0010-public-repo-hygiene.md) | Public-repo hygiene | Accepted |
| [11](0011-seen-means-you-acted.md) | Seen means you acted on it | Accepted |
| [12](0012-scope-and-non-goals.md) | Scope and non-goals | Superseded by 15 |
| [13](0013-rexboard-ships-the-hooks-it-relies-on.md) | rexboard ships the hooks it relies on | Accepted; amended |
| [14](0014-writes-to-user-config-are-explicit-reversible-and-fail-open.md) | Writes to user config are explicit, reversible and fail open | Accepted |
| [15](0015-scope-and-non-goals.md) | Scope and non-goals | Accepted |

## Adding or changing one

1. Copy the shape of any record: a title, `**Status:**` with a date, then Context, Decision, Consequences and Supersedes. Keep it under about 40 lines.
2. Number it next (`0016-…`), with a short kebab-case name.
3. A record that replaces another says so under Supersedes, and the old one's status becomes `Superseded by N`. Records are not deleted.
4. A change that narrows or adds to a record without reversing it may edit that record instead: add `Amended <date>` to its status and say what changed under Supersedes.
5. Add it to the table above, in the same PR as the change it decides.

Status is one of `Proposed`, `Accepted` or `Superseded by N`, and may add `Amended <date>`.
