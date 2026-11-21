# 20. The board can show only the groups you keep

**Status:** Accepted, 2026-11-22

## Context

A grouped view can hold many groups. The `session` view on a fleet of three hosts shows about 21, and on a given day the user works in a few of them. The board narrows rows two ways today: the `/` text (and-ed `key:value` terms) and one `f` status. Each has a word in the top bar, a key that clears it, a key in `board.json` ([19](0019-the-board-opens-where-it-was-left.md)) and a flag that `ls` shares ([5](0005-the-cli-is-the-api.md)). Neither fits a choice of groups that should last while the user searches. The `/` line is also the ad hoc search, so clearing a search would clear the choice, and its terms match by substring, so `session:api` cannot name one session on one host. Marks ([18](0018-rows-can-be-marked.md)) mean "this pane matters": keeping a group by marking it would star every row in it and bury the marks that matter. Folds still draw a header for every group, and `n`, `'`, `f` and a new NEEDS YOU row open them by design.

## Decision

- **A kept group** is a group of a grouped view: a status group, a harness, a host, a session on its host, or one value of an extension field. It is known by its labels (a session by its label and host), never by an id, so it survives a restart and shows again when its rows come back. A kept group with no rows stays kept.
- **One set per grouping, one switch.** The board holds a set of kept groups for each grouping, and one switch, only-kept. `+` on a row keeps or drops that row's group in the current view's grouping. `f+` turns only-kept on and off. While it is on, a row shows only when its group is kept in every grouping that has kept groups: the sets hold across views and are and-ed, as `/` terms are. The `table` view has no groups, so `+` there changes nothing and says so.
- **A filter beside the others, not inside them.** Only-kept and-s with the `f` status and the `/` text in the one predicate every list uses. `esc`, `ff` and clearing `/` leave it alone. Only `+`, `f+` and `:unkeep` change it. `:unkeep` drops every kept group, one that is not on the board included.
- **It never hides silently.** A kept header shows `+` before its name, in the `marked` look, and the name reads without colour ([8](0008-one-style-system.md), principle 4). `+` on a row says what it did, naming the group in full (`api@studio kept · f+ shows only kept`, `api@studio dropped`). While only-kept is on, the top bar names each grouping with kept groups and how many (`only 3 sessions`). It also counts the NEEDS YOU rows outside the kept groups, whatever the `f` status and the `/` text (`2 need you elsewhere`, in the `strong` look), since the board's job is to show who needs you ([15](0015-scope-and-non-goals.md)). When the top bar does not fit, the clock, the counts and the hosts drop first, as today. Then the kept words shorten (`3 kept`, or `only kept` for two groupings). The status word and the need-you count never drop. An empty list says `no rows in the kept groups · f+ shows all`, and with a status on as well `no working rows in the kept groups · ff clears · f+ shows all`. `:unkeep` says how many it dropped (`dropped 3 kept groups`).
- **Kept for the next run.** `board.json` gains `keep` (per grouping, the kept groups' labels) and `only_kept` (a boolean), saved on a user change by [19](0019-the-board-opens-where-it-was-left.md)'s rules. The board and `board --once` share them. `pick` never reads `board.json` ([19](0019-the-board-opens-where-it-was-left.md): a pick is a fresh list). It takes `+` and `f+` for its run, opens with nothing kept unless `--keep` is given, and saves nothing.
- **The CLI says the same.** `ls`, `board` and `pick` take `--keep GROUPING=GROUP`, repeatable, so `ls` lists what the board would. A flag wins for the run and is not saved. A grouping that names no view fails with `USAGE`. `ls --json` gains no field.
- **The keys** come from the one table ([6](0006-vim-keymap-from-one-table.md)): `+` and `f`, then `+`, were unbound. `f`'s footer lists `+ kept` with its count of rows in the kept groups. The board's idle footer gains no hint for it, so `n` stays on it at 80 columns; the top bar, the empty message and `?` say how to leave.

## Not

- **A user-defined view or layout.** It changes which rows show, not how they are grouped or drawn ([15](0015-scope-and-non-goals.md)). Named or saved sets, and a `config.toml` table for them, stay out.
- **A grammar in the `/` text,** a mark or a fold. The reasons are in Context.
- **Following a session across a rename or a renumber.** A renamed session is a new group and has to be kept again, as a favorite has to ([18](0018-rows-can-be-marked.md)).
- **State in serve.** `next` and `ls` without `--keep` read no kept groups.

## Consequences

- A third thing narrows the list, and the top bar names it.
- A NEEDS YOU row outside the kept groups is hidden but counted. `n` goes only to rows shown. A user who wants every row that needs them turns `f+` off.
- `board.json` gains two keys. An older rexboard keeps keys it does not know ([19](0019-the-board-opens-where-it-was-left.md)), so nothing is lost in either direction. The flag name and the two key names are contracts once released ([5](0005-the-cli-is-the-api.md)).

## Supersedes

None. It amends [19](0019-the-board-opens-where-it-was-left.md): `board.json` gains `keep` and `only_kept`.
