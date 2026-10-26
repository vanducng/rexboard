# Decisions

Each record says what rexboard decided, why, and what it costs. They are short on purpose: the docs say how things work, and these say why they work that way. Change one of these directions only by proposing a new record.

| # | Decision | Status |
| --- | --- | --- |
| [1](0001-reach-rex-only-through-the-rex-cli.md) | Reach Rex only through the `rex` CLI | Accepted |
| [2](0002-one-serve-daemon-many-clients.md) | One serve daemon, many clients | Accepted |
| [3](0003-the-agent-tells-rex-rexboard-shows-it.md) | The agent tells Rex, rexboard shows it | Accepted |
| [4](0004-extensions-not-a-built-in-core.md) | Extensions, not a built-in core | Accepted |
| [5](0005-the-cli-is-the-api.md) | The CLI is the API | Accepted |
| [6](0006-vim-keymap-from-one-table.md) | A vim keymap from one table | Accepted |
| [7](0007-acting-on-panes-is-guarded.md) | Acting on panes is guarded | Accepted |
| [8](0008-one-style-system.md) | One style system | Accepted |
| [9](0009-release-and-install.md) | Release and install | Accepted |
| [10](0010-public-repo-hygiene.md) | Public-repo hygiene | Accepted |
| [11](0011-seen-means-you-acted.md) | Seen means you acted on it | Accepted |
| [12](0012-scope-and-non-goals.md) | Scope and non-goals | Accepted |

## Adding or changing one

1. Copy the shape of any record: a title, `**Status:**` with a date, then Context, Decision, Consequences and Supersedes. Keep it under about 40 lines.
2. Number it next (`0013-…`), with a short kebab-case name.
3. A record that replaces another says so under Supersedes, and the old one's status becomes `Superseded by N`. Records are not deleted.
4. Add it to the table above, in the same PR as the change it decides.

Status is one of `Proposed`, `Accepted` or `Superseded by N`.
