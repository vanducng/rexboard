# 7. Acting on panes is guarded

**Status:** Accepted, 2026-10-26

## Context

Answering an agent from the board saves a jump, and closing a finished pane saves a trip. Both write to a pane you are not looking at. The failure cases are real: a fast `j`/`k` plus a key hits the wrong agent; a digit typed into an idle prompt becomes text; `2` can grant a permanent rule; text meant for an agent often lands in the board's own pane.

## Decision

- Acting is an explicit mode, never a key on the plain board. `a` (or `i`) opens answer mode with a red bar naming the target; `dd` asks before a close, and only `y` closes.
- Answer mode is bound to the row's host and pane, not its place in the list.
- One `rex do` per send holds the whole guard and the send. Before anything goes out it re-checks, on the pane's host: the same state and kind, an agent (not a shell) in front, and the whole dialog or input line still on screen, line for line. Any change refuses and sends nothing.
- An option that grants something for later needs the same digit twice. Text goes as a paste then Enter, never into a dialog. Esc and Ctrl+C are never sent.
- Typing guard: `a`, `i` and `dd` act only when pressed on their own, 300 ms after the key before; a first key within 300 ms of opening answer mode leaves it, and a key within 300 ms of `dd`'s question cancels it.
- The board never answers or closes its own pane.
- The CLI's `answer` and `close` name the pane by its key and go through the same read and re-check.
  - Pass what you read: `answer` takes the `--dialog` id `show --json` gave, and refuses when the pane shows another dialog or input line, even one with the same state, kind and options. On the board the screen drawn is the screen checked; a script reads and acts minutes apart.
  - A lasting option takes `--confirm` instead of a second press. A close takes `--yes`, or a `y` on a terminal, and without a terminal never asks.
  - Neither acts on the caller's own pane: the one in `REX_BLOCK` outside tmux. Inside tmux `REX_BLOCK` is the pane that started the tmux server, so `close` refuses any local pane running tmux, which may be the caller's; `answer` skips the check there, since a pane running the command is not waiting on a dialog.
  - The typing guard is the board's alone: a command line is not typed into the wrong place by a stray key.
- Live tests write only to throwaway `zz-rexboard-probe-*` panes, never to real ones.

## Consequences

- A refusal costs a second try; a wrong send would cost an agent's work. The guard picks the first.
- Options are read for Claude Code's dialogs; another harness gets line replies only, until its dialogs are probed.
- Interrupting an agent (sending Esc) stays out until it can be just as explicit.

## Supersedes

Sending `y`, `n` and digits straight from the board, and an `i` prompt with `rex send`.
