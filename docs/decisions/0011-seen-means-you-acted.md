# 11. Seen means you acted on it

**Status:** Accepted, 2026-10-25

## Context

A finished agent should stay in NEEDS YOU until you have dealt with it, then get out of the way. Rex reports which tab is active, but not which pane has focus, so "you looked at it" cannot be observed.

## Decision

- A row becomes seen when you act on it: a jump (`enter`, `next`, `pick`, `jump`), `m` or `rexboard seen`, `:seen` for every shown done row, an answer sent from the board, or `n`/`tab` off a done row whose card showed for at least a second. The agent starting new work marks it seen too.
- Reading a card does not mark a row seen, and neither does a tab becoming active.
- A blocked or erroring row stays in NEEDS YOU while seen, until it moves on.
- A row is unseen again each time it enters `done`, `blocked` or `error`. serve keeps `seen` across restarts.

## Consequences

- NEEDS YOU is a queue you work through, and `next` walks it.
- A row you glanced at but did not act on stays in the queue, which is the point.

## Supersedes

None.
