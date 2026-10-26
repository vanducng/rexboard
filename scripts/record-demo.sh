#!/usr/bin/env bash
# Records the README demo and the feature casts: the board on a private, throwaway Rex server
# whose panes play a scripted day, driven with tmux keys.
#
#   scripts/record-demo.sh              every scene
#   scripts/record-demo.sh card cli     only these
#
# `hero` writes docs/assets/demo.cast and demo.gif; every other scene writes
# docs/assets/casts/SCENE.cast and SCENE.gif.
#
# Nothing here touches your own Rex server or rexboard: each scene runs Rex, rexboard serve and
# the board under its own temporary HOME and XDG dirs, and stops every one of them at the end.
# The clipboard is a file in that temporary dir, so `yy` never reaches yours.
#
# REXBOARD_DEMO_DENY may name a file of strings (one per line) that must not appear in a cast,
# such as your real host, session and tab labels. /Users/, /home/, /tmp/, /private/, your user
# name, this machine's name and your home path are always refused.
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$here/.." && pwd)
all=(hero board card answer full moving filter views sort close extensions cli icons)
if [[ $# -gt 0 ]]; then scenes=("$@"); else scenes=("${all[@]}"); fi
for scene in "${scenes[@]}"; do
  case " ${all[*]} " in
    *" $scene "*) ;;
    *) echo "no scene named $scene; scenes: ${all[*]}" >&2; exit 2 ;;
  esac
done
tmux_socket=zzdemo
if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
for tool in rex asciinema agg tmux python3 cargo base64 jq; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
tmux -L "$tmux_socket" has-session 2>/dev/null && { echo "tmux server $tmux_socket is already running" >&2; exit 1; }
(cd "$repo" && cargo build --quiet)
board="$repo/target/debug/rexboard"
mkdir -p "$repo/docs/assets/casts"
# Refused in every cast, besides the fixed list and REXBOARD_DEMO_DENY.
denied=$(printf '%s\n' "$(id -un)" "$(hostname -s)" "$HOME")
# Dracula-like colours, with palette 8 (the board's selected row) a quiet raise over the page.
theme=282a36,f8f8f2,21222c,ff5555,50fa7b,f1fa8c,8be9fd,ff79c6,8be9fd,f8f8f2,3a3f52,ff6e6e,69ff94,ffffa5,a4ffff,ff92df,a4ffff,ffffff

# One scene, in its own subshell: a private Rex server, the demo panes, one recording.
record_scene() {
  local scene=$1
  local cols=140 rows=30 title="" command="" kind=board fonts="" early=0 fps=8
  case "$scene" in
    hero) title="rexboard: an attention board for agent panes across Rex hosts" ;;
    board) title="rexboard: every agent's state at a glance" ;;
    card) cols=120 rows=26 title="rexboard: the card, its facts, and scrolling" ;;
    answer) early=1 fps=6 title="rexboard: answer an agent from the board" ;;
    full) title="rexboard: the full view of a pane, live" ;;
    # These two redraw the whole list on most keys, so a smaller, slower GIF keeps them light.
    moving) cols=120 rows=26 fps=6 title="rexboard: moving with vim keys" ;;
    filter) cols=120 rows=26 fps=6 title="rexboard: filter by text, key and status" ;;
    views) cols=120 rows=26 fps=6 title="rexboard: views" ;;
    sort) cols=120 rows=26 fps=6 title="rexboard: sorting by a column" ;;
    close) title="rexboard: copy, and close a pane after asking" ;;
    extensions) kind=shell title="rexboard: a column from an extension" ;;
    cli) kind=shell cols=120 rows=28 title="rexboard: the CLI as an API" ;;
    icons) kind=shell title="rexboard: words or Nerd Font icons"
      # A Nerd Font with the 3.5 glyphs (cod-claude and the rest) as the fallback for icons.
      fonts="JetBrains Mono,FiraCode Nerd Font Mono" ;;
  esac

  # A short root, since unix socket paths are limited to about 100 bytes. Not local: the EXIT
  # trap that cleans it up runs after this function returns.
  root=$(mktemp -d /tmp/rbdemo.XXXX)
  mkdir -p "$root/home/.config" "$root/run" "$root/state" "$root/data" "$root/cache" "$root/tmp" "$root/bin"
  chmod 700 "$root/run"
  # The config dir sits in the private home, so `ext ls` shows it as `~/.config/...`.
  export HOME="$root/home" XDG_RUNTIME_DIR="$root/run" XDG_CONFIG_HOME="$root/home/.config" \
    XDG_STATE_HOME="$root/state" XDG_DATA_HOME="$root/data" XDG_CACHE_HOME="$root/cache" TMPDIR="$root/tmp" \
    PATH="$root/bin:$PATH"
  unset REX_SERVER REX_SESSION REX_BLOCK REX_CLIENT REXBOARD_ICONS REXBOARD_REDUCED_MOTION
  # Set only once the Rex server is proven to be the private one; nothing else is ever stopped.
  ours=0
  server_pid=
  cleanup() {
    tmux -L "$tmux_socket" kill-server 2>/dev/null || true
    # The private serve, whose socket lives under $root/state; XDG_STATE_HOME keeps --stop there.
    "$board" serve --stop >/dev/null 2>&1 || true
    if [[ "$ours" == 1 && -n "$server_pid" ]] && ps -o command= -p "$server_pid" | grep -qF "$root/"; then
      kill "$server_pid" 2>/dev/null || true
    fi
    rm -rf "$root"
  }
  trap cleanup EXIT

  # Start the private server, then prove it is ours: its socket and its process both live under
  # $root. Anything else is someone else's server, so refuse without touching it.
  rex server start >/dev/null 2>&1 || true
  read -r endpoint server_pid < <(rex server status --json 2>/dev/null |
    python3 -c 'import json, sys; d = json.load(sys.stdin); print(d.get("endpoint", "-"), d.get("pid", "-"))' ||
    echo "- -")
  if [[ "$endpoint" != "unix://$root/"* ]] || ! ps -o command= -p "$server_pid" 2>/dev/null | grep -qF "$root/"; then
    server_pid=
    echo "the Rex server rex reaches is not the private one under $root; refusing to record" >&2
    exit 1
  fi
  ours=1
  # A second gate: the private server has no hosts of its own.
  hosts=$(rex hosts --json | python3 -c 'import json, sys; print(len(json.load(sys.stdin).get("hosts") or []))')
  [[ "$hosts" == 0 ]] || { echo "the demo Rex server has hosts; refusing to record" >&2; exit 1; }

  # The repo's example extension gives each row a PROJECT column: its session label up to the `-`.
  mkdir -p "$XDG_CONFIG_HOME/rexboard/extensions"
  cp -R "$repo/examples/extensions/session-prefix" "$XDG_CONFIG_HOME/rexboard/extensions/"
  printf '[extensions]\nenabled = ["session-prefix"]\n' >"$XDG_CONFIG_HOME/rexboard/config.toml"
  # `rexboard` by name for the shell scenes, and a clipboard that is a file here.
  ln -s "$board" "$root/bin/rexboard"
  for copy in pbcopy wl-copy xclip; do
    printf '#!/bin/sh\ncat >"%s/clipboard"\n' "$root" >"$root/bin/$copy"
    chmod +x "$root/bin/$copy"
  done

  # Answer mode sends only to a pane whose foreground is an agent, so the demo panes run under
  # copies of bash named like the four harnesses' processes.
  grok=grok-1.0.0-darwin-arm64
  for name in claude codex pi "$grok"; do cp /bin/bash "$root/bin/$name"; done
  cat >"$root/pane.sh" <<'PANE'
# One demo pane: wait for the go file, then play a timeline of titles, screens and status reports.
seat=$1 go=$2
# Each harness reports under its own app name.
case "$seat" in
  seat-b | seat-e) app=codex ;;
  seat-f) app=grok-build ;;
  seat-g) app=pi ;;
  *) app=claude-code ;;
esac
title() { printf '\033]2;%s\007' "$1"; }
report() { # state, message (lines joined with " ⏎ "), [kind]
  local kind=""
  [ -n "${3:-}" ] && kind=":kind=$3"
  printf '\033]7501;state=%s%s:app=%s:msg=%s\007' "$1" "$kind" "$app" "$(printf '%s' "$2" | base64)"
}
# at N: wait until N seconds after the go file appears.
now=0
at() { while [ ! -e "$go" ]; do sleep 0.1; done; sleep $(($1 - now)); now=$1; }
rule='────────────────────────────────────────────────────────────'
case "$seat" in
  seat-a)
    title "⠋ seat-a@demo"; printf '● Fixing the nested parser case.\n\n'
    report working "Fixing the nested parser case"
    at 5
    printf '● Bash(cargo test --workspace --features full)\n\n'
    printf ' Bash command\n\n   cargo test --workspace --features full\n   Run the full test suite\n\n'
    printf ' Do you want to proceed?\n'
    printf ' ❯ 1. Yes\n   2. Yes, and don'"'"'t ask again for cargo test commands in /work/api\n'
    printf '   3. No, and tell Claude what to do differently\n'
    report blocked "Bash wants to run: cargo test --workspace --features full" permission
    # The board's answer: one key, read here with nothing else in front.
    key=; while [ "$key" != 1 ]; do read -r -s -n 1 key; done
    clear; title "⠋ seat-a@demo"; printf '● Running the full test suite.\n  running 12 tests\n'
    report working "Running the full test suite"
    sleep 9; title "✳ seat-a@demo"; printf '  test result: ok. 12 passed\n'
    report done "Tests pass: 12 passed ⏎ The nested parser case is fixed and covered by a new test." ;;
  seat-b)
    title "✳ seat-b@demo"; printf '● Posted 2 comments on the parser change.\n'
    report done "Review posted with 2 comments ⏎ Both are about naming in src/parser.rs; nothing blocks the merge. ⏎ 1. parse_block: rename depth to nesting, since it counts open blocks, not lines. ⏎ 2. tokens: the comment filter reads better as its own small function. ⏎ Tests: 12 passed, 0 failed, on the branch head. ⏎ Clippy and fmt are clean. ⏎ The change is 48 lines in two files. ⏎ Next step is yours: answer the two comments, then merge." ;;
  seat-c)
    title "✳ seat-c@demo"
    printf ' ☐ Test runner\n\nWhich test runner should the web package use?\n\n'
    printf '❯ 1. vitest\n     Fast, native ESM, works with Vite\n  2. jest\n     Familiar, more plugins\n  3. Type something.\n\n'
    printf 'Enter to select · ↑/↓ to navigate · Esc to cancel\n'
    report blocked "Which test runner should the web package use? ⏎ 1. vitest: fast, native ESM ⏎ 2. jest: familiar, more plugins" question ;;
  seat-d)
    # A coloured diff that grows while the full view follows it, then a done report.
    title "⠋ seat-d@demo"; report working "Updating the style docs"
    printf '\033[1mdiff --git a/docs/tui.md b/docs/tui.md\033[0m\n\033[36m@@ -36,6 +36,18 @@ ## Style\033[0m\n'
    printf ' One table says how each state looks.\n'
    printf '\033[31m-  Glyphs: blocked, error, done, working, idle.\033[0m\n'
    at 0
    while IFS= read -r line; do
      sleep 1.5
      case "$line" in
        +*) printf '\033[32m%s\033[0m\n' "$line" ;;
        -*) printf '\033[31m%s\033[0m\n' "$line" ;;
        @*) printf '\033[36m%s\033[0m\n' "$line" ;;
        *) printf '%s\n' "$line" ;;
      esac
    done <<'DIFF'
+| State    | Shape | Colour |
+|----------|-------|--------|
+| blocked  | !     | red    |
+| question | ?     | red    |
+| error    | ✗     | red    |
+| done     | ✓     | green  |
+| working  | ⠹     | yellow |
+| idle     | ○     | dim    |

-Spinners turn once a second.
+Working rows spin a braille dot every 160 ms.
+Reduced motion holds it still as ⠿.
@@ -58,4 +70,9 @@ ## Turn card
 Set it in tui.json:
+
+```json
+{ "motion": "reduced" }
+```
+Or for one run: REXBOARD_REDUCED_MOTION=1
DIFF
    sleep 0.3; title "✳ seat-d@demo"
    report done "Style docs updated ⏎ A shape table and a reduced motion note in docs/tui.md." ;;
  seat-e)
    title "✳ seat-e@demo"; printf 'Ready.\n' ;;
  seat-f)
    title "✳ seat-f@demo"; printf 'Waiting for input.\n'
    report idle "Waiting for input" ;;
  seat-g)
    title "✳ seat-g@demo"; printf 'error: 2 errors in the release build\n'
    report error "Build failed: 2 errors in the release build" ;;
  seat-h)
    # An idle agent at its input line, which takes one reply as a paste and Enter.
    title "✳ seat-h@demo"; printf '● The parser handles nested blocks now.\n\n'
    printf '%s\n❯\302\240\n%s\n' "$rule" "$rule"
    printf '\033[2A\033[3G'
    report idle "The parser handles nested blocks now"
    IFS= read -r reply
    clear; title "⠋ seat-h@demo"; printf '❯ %s\n\n● Adding a test for the empty input.\n' "$reply"
    report working "Adding a test for the empty input" ;;
esac
# Hold the pane with a builtin, so the harness's own process stays in front.
mkfifo "$go.$seat"
read -r -t 600 <>"$go.$seat" || :
PANE

  go="$root/go"
  # pane HARNESS SEAT REX-ARGS...: one demo pane, under that harness's process name.
  pane() { local bin=$1 seat=$2; shift 2; rex "$@" -- "$root/bin/$bin" "$root/pane.sh" "$seat" "$go" >/dev/null; }
  pane claude seat-a new api-gateway --window build
  pane codex seat-b run -s api-gateway --focus=false
  pane pi seat-g run -s api-gateway --focus=false
  pane claude seat-c new web-app --window deploy
  pane claude seat-d run -s web-app --focus=false
  pane codex seat-e run -s web-app --focus=false
  pane "$grok" seat-f run -s web-app --focus=false
  if [[ "$scene" == answer ]]; then
    pane claude seat-h run -s web-app --focus=false
  fi
  sleep 1
  # Pane and tab labels, by the seat each pane titles itself with.
  rex do - >/dev/null <<'LUA'
local tabs = { ["seat-a"] = "build", ["seat-b"] = "review", ["seat-c"] = "deploy",
  ["seat-d"] = "docs", ["seat-e"] = "notes", ["seat-f"] = "infra", ["seat-g"] = "release",
  ["seat-h"] = "parser" }
rex.call("session.attach_all", { event_scope = "session" })
for _, s in ipairs(rex.call("session.list").sessions or {}) do
  for _, b in ipairs((rex.call("session.list_blocks", { session_id = s.session_id }) or {}).blocks or {}) do
    local t = rex.block.call("com.superlogical.terminal", "title", { session_id = s.session_id, block_id = b.block_id })
    local seat = t and t.title and t.title:match("(seat%-%a)@demo")
    if seat then
      rex.call("session.set_block_label", { session_id = s.session_id, block_id = b.block_id, label = seat })
      rex.call("session.set_window_label", { session_id = s.session_id, window_id = b.window_id, label = tabs[seat] })
    end
  end
end
LUA

  # Start serve now, so a cast opens on the board, not on `connecting to serve`.
  "$board" ls >/dev/null 2>&1 || true

  # The shell scenes type their commands at a fake prompt; nothing of your own shell shows.
  cat >"$root/session.sh" <<'SESSION'
scene=$1
prompt() { printf '\033[1;35m$\033[0m '; }
run() {
  local line="$*" i
  prompt
  for ((i = 0; i < ${#line}; i++)); do printf '%s' "${line:i:1}"; sleep 0.04; done
  sleep 0.5; printf '\n'
  eval "$line"
  sleep 1.2
}
key() { rexboard ls --json --all | jq -r --arg pane "$1" '.[] | select(.pane == $pane) | .key'; }
clear
case "$scene" in
  extensions)
    run 'rexboard ext ls'
    run 'rexboard ext test session-prefix'
    run 'rexboard' ;;
  cli)
    run 'rexboard ls --sort harness'
    run 'rexboard ls --status question'
    c=$(key seat-c)
    run "rexboard show $c"
    run "rexboard show $c --json | jq '{state, kind, where, msg}'"
    run 'rexboard hosts'
    run 'rexboard seen --all-done' ;;
  icons)
    run 'rexboard'
    run 'REXBOARD_ICONS=nerd rexboard' ;;
esac
prompt
sleep 1.5
SESSION

  local cast="$root/demo.cast"
  if [[ "$kind" == board ]]; then
    command="$board board"
    # A scene that starts on a blocked agent lets the timeline run first.
    if [[ "$early" == 1 ]]; then touch "$go"; sleep 6; fi
  else
    command="bash $root/session.sh $scene"
    # The shell scenes record a settled board: the first reports are in.
    touch "$go"
    sleep 7
  fi
  tmux -L "$tmux_socket" new-session -d -x "$cols" -y "$rows" -s demo \
    "asciinema rec '$cast' --overwrite --quiet --window-size ${cols}x${rows} --idle-time-limit 2 \
     --title \"$title\" --command '$command'"
  tmux -L "$tmux_socket" set-option -t demo status off >/dev/null
  k() { local key; for key in "$@"; do tmux -L "$tmux_socket" send-keys -t demo "$key"; sleep 0.35; done; }
  lit() { local i; for ((i = 0; i < ${#1}; i++)); do tmux -L "$tmux_socket" send-keys -t demo -l "${1:i:1}"; sleep 0.07; done; }
  # A click on the column header cell named $1, as the terminal sends it (SGR, 1-based).
  click_header() {
    local at x y
    at=$(tmux -L "$tmux_socket" capture-pane -p -t demo | python3 -c '
import sys
for y, line in enumerate(sys.stdin, 1):
    if "HARNESS" in line and sys.argv[1] in line:
        print(line.index(sys.argv[1]) + 2, y)
        break' "$1")
    [[ -n "$at" ]] || { echo "$scene: no $1 header" >&2; exit 1; }
    x=${at% *} y=${at#* }
    tmux -L "$tmux_socket" send-keys -t demo -l $'\e'"[<0;$x;${y}M"$'\e'"[<0;$x;${y}m"
  }
  wait_for() {
    local _
    for _ in $(seq 1 $((${2:-10} * 10))); do
      tmux -L "$tmux_socket" capture-pane -p -t demo 2>/dev/null | grep -qF -- "$1" && return 0
      sleep 0.1
    done
    echo "$scene: '$1' never showed" >&2
    exit 1
  }
  if [[ "$kind" == board ]]; then
    wait_for 'NEEDS YOU'
    sleep 1.5
    touch "$go"
  fi
  # Every opener (`a`, the first `d`) comes after a pause, as the typing guard asks.
  case "$scene" in
    hero)
      # The spinners and every shape; a done row's card, then `m` marks it seen; the permission
      # row's ask, `a` on its own, `2` once to show that a lasting option asks again, then `1`
      # answers it from the board; `z` shows what may follow it and `za` folds a group and opens
      # it again; `/docs` narrows the list to the docs row, whose live, coloured full view
      # follows its diff; `q` back as it finishes, then `dd` asks before `y` closes it.
      sleep 2.5; k 3; sleep 2.2
      k m; sleep 1.2
      k 2; sleep 2
      k a; sleep 3; k 2; sleep 2; k 1; sleep 2.5
      k z; sleep 1.5; k a; sleep 1.2; k z a; sleep 0.8
      k / d o c s Enter; sleep 1.5; k Space; sleep 4; k q; sleep 1.2
      k d d; sleep 3; k y; sleep 1.8; k Escape; sleep 1.5 ;;
    board)
      # The groups and every shape, the spinner, then the key table.
      sleep 3; k j; sleep 0.8; k j; sleep 0.8; k j; sleep 1.5
      k '?'; sleep 4; k q; sleep 1.5 ;;
    card)
      # A long done message: the bar holds its row, ctrl-e and ctrl-y scroll the text under it,
      # and the next row's card starts at its top.
      sleep 2; k 3; sleep 2.5
      k C-e; sleep 0.6; k C-e; sleep 0.6; k C-e; sleep 0.6; k C-e; sleep 1.5
      k C-y; sleep 0.6; k C-y; sleep 1.5
      k j; sleep 2.5 ;;
    answer)
      # The permission row: `a`, `2` once (a lasting option asks again), `1`. Then a typed reply
      # to an idle agent at its input line.
      wait_for 'permission' 15
      sleep 1; k 2; sleep 1.2
      k a; sleep 2; k 2; sleep 1.6; k 1; sleep 1.8
      k / s e a t - h Enter; sleep 0.8
      k a; sleep 1.2; lit 'Add a test for the empty input'; sleep 0.5; k Enter; sleep 2 ;;
    full)
      # The docs row's pane, live and in colour, then back.
      k / d o c s Enter; sleep 1.5; k Space; sleep 7; k q; sleep 1.5 ;;
    moving)
      # Moving by row, by screen and by group, the rows that need you, and the keys a prefix
      # takes.
      sleep 1.5; k j j j; k G; sleep 0.5; k g g; sleep 0.5; k L; k H; sleep 0.5
      k ']' ']' '['; sleep 0.5; k n n; sleep 0.8
      k z; sleep 1.6; k Escape
      k g; sleep 1.6; k Escape; sleep 0.8 ;;
    filter)
      # A text filter and a key:value one, then one status.
      sleep 1.5; k /; lit 'parser'; k Enter; sleep 1.2; k Escape
      k /; lit 'state:done'; k Enter; sleep 1.2; k Escape
      k f; sleep 1.4; k q; sleep 1.4; k f f; sleep 0.8 ;;
    views)
      # The project view from the extension, then the harness and host views by name.
      sleep 1.5; k V; sleep 1.6; k :; lit 'harness'; k Enter; sleep 1.8
      k :; lit 'host'; k Enter; sleep 1.6 ;;
    sort)
      # `o` lists the letters; age, its flip, harness; in the table view a header click sorts
      # and a second flips; `esc` clears it.
      sleep 1.5; k o; sleep 1.8; k a; sleep 1.6; k o a; sleep 1.6; k o h; sleep 1.8
      k :; lit 'table'; k Enter; sleep 1.8
      click_header HOST; sleep 1.6; click_header HOST; sleep 1.6
      k Escape; sleep 1.4 ;;
    close)
      # Copy a row's message and where it is, then `dd` asks and `y` closes the pane.
      sleep 2; k 3; sleep 1.5
      k y y; sleep 1.8; k y p; sleep 1.8
      sleep 0.5; k d d; sleep 3; k y; sleep 2.5 ;;
    extensions)
      # The extension's own commands, then its PROJECT column and its view on the board.
      wait_for 'NEEDS YOU' 30
      sleep 2; k v; sleep 0.5; k v; sleep 0.5; k v; sleep 0.5; k v; sleep 3; k q ;;
    cli) ;;
    icons)
      # The same card with words, then with Nerd Font icons.
      wait_for 'NEEDS YOU' 30
      sleep 1; k / s e a t - d Enter; sleep 2.5; k q
      sleep 1; wait_for 'NEEDS YOU' 30
      sleep 1; k / s e a t - d Enter; sleep 3; k q ;;
  esac
  if [[ "$kind" == board ]]; then k q; fi
  local _
  for _ in $(seq 1 300); do tmux -L "$tmux_socket" has-session -t demo 2>/dev/null || break; sleep 0.1; done

  python3 - "$cast" "$kind" "${REXBOARD_DEMO_DENY:-}" "$denied" <<'PY'
import json, sys
path, kind, deny_file, denied = sys.argv[1:5]
lines = open(path).read().splitlines()
header = json.loads(lines[0])
header["command"] = "rexboard board" if kind == "board" else "rexboard"
header.pop("env", None)
header.get("term", {}).pop("type", None)
lines[0] = json.dumps(header)
# A board scene ends on the board, not on the screen it leaves behind when it quits.
if kind == "board":
    for i, line in enumerate(lines[1:], 1):
        if line.strip() and "\x1b[?1049l" in json.loads(line)[2]:
            lines = lines[:i]
            break
blob = "\n".join(lines)
deny = ["/Users/", "/home/", "/tmp/", "/private/", "claude.ai", ".ts.net"]
deny += [line.strip() for line in denied.splitlines() if line.strip()]
if deny_file:
    deny += [line.strip() for line in open(deny_file) if line.strip()]
found = sorted({token for token in deny if token in blob})
if found:
    sys.exit(f"private data in the cast: {len(found)} denied string(s); not writing it")
open(path, "w").write(blob + "\n")
PY
  local out="$repo/docs/assets/casts/$scene"
  if [[ "$scene" == hero ]]; then out="$repo/docs/assets/demo"; fi
  cp "$cast" "$out.cast"
  local font=()
  if [[ -n "$fonts" ]]; then font=(--font-family "$fonts"); fi
  agg --theme "$theme" --font-size 12 --idle-time-limit 2 --fps-cap "$fps" --last-frame-duration 3 "${font[@]}" \
    "$out.cast" "$out.gif" >/dev/null 2>&1
  echo "wrote ${out#"$repo"/}.cast and .gif"
}

for scene in "${scenes[@]}"; do
  (record_scene "$scene")
done
