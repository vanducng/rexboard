#!/usr/bin/env bash
# Records the README demo: the board on a private, throwaway Rex server whose panes play a
# scripted day, driven with tmux keys. Writes docs/assets/demo.cast and docs/assets/demo.gif.
#
# Nothing here touches your own Rex server or rexboard: Rex, rexboard serve and the board all
# run under a temporary HOME and XDG dirs, and every one of them is stopped at the end.
#
# REXBOARD_DEMO_DENY may name a file of strings (one per line) that must not appear in the cast,
# such as your real host, session and tab labels. /Users/, /home/, /tmp/ and /private/ are
# always refused.
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$here/.." && pwd)
# 140x30: SESSION, WINDOW and BLOCK get their own columns, an extension field gets one, and the
# turn card has room under every row.
cols=140
rows=30
tmux_socket=zzdemo
if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
for tool in rex asciinema agg tmux python3 cargo base64; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
tmux -L "$tmux_socket" has-session 2>/dev/null && { echo "tmux server $tmux_socket is already running" >&2; exit 1; }
(cd "$repo" && cargo build --quiet)
board="$repo/target/debug/rexboard"

# A short root, since unix socket paths are limited to about 100 bytes.
root=$(mktemp -d /tmp/rbdemo.XXXX)
mkdir -p "$root/home" "$root/run" "$root/config" "$root/state" "$root/data" "$root/cache" "$root/tmp"
chmod 700 "$root/run"
export HOME="$root/home" XDG_RUNTIME_DIR="$root/run" XDG_CONFIG_HOME="$root/config" \
  XDG_STATE_HOME="$root/state" XDG_DATA_HOME="$root/data" XDG_CACHE_HOME="$root/cache" TMPDIR="$root/tmp"
unset REX_SERVER REX_SESSION REX_BLOCK REX_CLIENT
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

# Answer mode sends only to a pane whose foreground is an agent, so the demo panes run under a
# copy of bash named like one.
mkdir -p "$root/bin"
cp /bin/bash "$root/bin/claude"
cat >"$root/pane.sh" <<'PANE'
# One demo pane: wait for the go file, then play a timeline of titles, screens and status reports.
seat=$1 go=$2
title() { printf '\033]2;%s\007' "$1"; }
report() { # state, message (lines joined with " ⏎ "), [kind]
  local kind=""
  [ -n "${3:-}" ] && kind=":kind=$3"
  printf '\033]7501;state=%s%s:app=claude-code:msg=%s\007' "$1" "$kind" "$(printf '%s' "$2" | base64)"
}
# at N: wait until N seconds after the go file appears.
now=0
at() { while [ ! -e "$go" ]; do sleep 0.1; done; sleep $(($1 - now)); now=$1; }
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
    report done "Review posted with 2 comments ⏎ Both are about naming in src/parser.rs; nothing blocks the merge." ;;
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
+Working rows spin a braille dot every 80 ms.
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
esac
exec sleep 600
PANE

go="$root/go"
agent="$root/bin/claude"
rex new api-gateway --window build -- "$agent" "$root/pane.sh" seat-a "$go" >/dev/null
rex run -s api-gateway --focus=false -- "$agent" "$root/pane.sh" seat-b "$go" >/dev/null
rex run -s api-gateway --focus=false -- "$agent" "$root/pane.sh" seat-g "$go" >/dev/null
rex new web-app --window deploy -- "$agent" "$root/pane.sh" seat-c "$go" >/dev/null
rex run -s web-app --focus=false -- "$agent" "$root/pane.sh" seat-d "$go" >/dev/null
rex run -s web-app --focus=false -- "$agent" "$root/pane.sh" seat-e "$go" >/dev/null
rex run -s web-app --focus=false -- "$agent" "$root/pane.sh" seat-f "$go" >/dev/null
sleep 1
# Pane and tab labels, by the seat each pane titles itself with.
rex do - >/dev/null <<'LUA'
local tabs = { ["seat-a"] = "build", ["seat-b"] = "review", ["seat-c"] = "deploy",
  ["seat-d"] = "docs", ["seat-e"] = "notes", ["seat-f"] = "infra", ["seat-g"] = "release" }
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

cast="$root/demo.cast"
tmux -L "$tmux_socket" new-session -d -x "$cols" -y "$rows" -s demo \
  "asciinema rec '$cast' --overwrite --quiet --window-size ${cols}x${rows} --idle-time-limit 2 \
   --title 'rexboard: an attention board for agent panes across Rex hosts' --command '$board board'"
tmux -L "$tmux_socket" set-option -t demo status off >/dev/null
k() { for key in "$@"; do tmux -L "$tmux_socket" send-keys -t demo "$key"; sleep 0.35; done; }
for _ in $(seq 1 100); do tmux -L "$tmux_socket" capture-pane -p -t demo 2>/dev/null | grep -q 'NEEDS YOU' && break; sleep 0.1; done
sleep 1.5
touch "$go"
# The spinners and every shape; a done row's card, then `m` marks it seen; the permission row's
# ask, `a` on its own, `2` once to show that a lasting option asks again, then `1` answers it from
# the board; `z` shows what may follow it and `za` folds a group and opens it again; `/docs`
# narrows the list to the docs row, whose live, coloured full view follows its diff; `q` back as
# it finishes, then `dd` asks before `y` closes it.
# Every opener (`a`, the first `d`) comes after a pause, as the typing guard asks.
sleep 2.5; k 3; sleep 2.2
k m; sleep 1.2
k 2; sleep 2
k a; sleep 3; k 2; sleep 2; k 1; sleep 2.5
k z; sleep 1.5; k a; sleep 1.2; k z a; sleep 0.8
k / d o c s Enter; sleep 1.5; k Space; sleep 4; k q; sleep 1.2
k d d; sleep 3; k y; sleep 1.8; k Escape; sleep 1.5
k q
for _ in $(seq 1 100); do tmux -L "$tmux_socket" has-session -t demo 2>/dev/null || break; sleep 0.1; done

python3 - "$cast" "${REXBOARD_DEMO_DENY:-}" <<'PY'
import json, sys
path, deny_file = sys.argv[1], sys.argv[2]
lines = open(path).read().splitlines()
header = json.loads(lines[0])
header["command"] = "rexboard board"
header.pop("env", None)
header.get("term", {}).pop("type", None)
lines[0] = json.dumps(header)
# End on the board, not on the screen it leaves behind when it quits.
for i, line in enumerate(lines[1:], 1):
    if line.strip() and "\x1b[?1049l" in json.loads(line)[2]:
        lines = lines[:i]
        break
blob = "\n".join(lines)
deny = ["/Users/", "/home/", "/tmp/", "/private/", "claude.ai", ".ts.net"]
if deny_file:
    deny += [line.strip() for line in open(deny_file) if line.strip()]
found = sorted({token for token in deny if token in blob})
if found:
    sys.exit(f"private data in the cast: {len(found)} denied string(s); not writing it")
open(path, "w").write(blob + "\n")
PY
cp "$cast" "$repo/docs/assets/demo.cast"
# Dracula-like colours, with palette 8 (the board's selected row) a quiet raise over the page.
theme=282a36,f8f8f2,21222c,ff5555,50fa7b,f1fa8c,8be9fd,ff79c6,8be9fd,f8f8f2,3a3f52,ff6e6e,69ff94,ffffa5,a4ffff,ff92df,a4ffff,ffffff
agg --theme "$theme" --font-size 12 --idle-time-limit 2 --fps-cap 8 --last-frame-duration 3 \
  "$repo/docs/assets/demo.cast" "$repo/docs/assets/demo.gif" >/dev/null 2>&1
echo "wrote docs/assets/demo.cast and docs/assets/demo.gif"
