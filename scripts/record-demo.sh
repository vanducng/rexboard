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
cols=120
rows=20
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
  if [[ -S "$root/run/rexboard.sock" ]]; then
    "$board" serve --stop >/dev/null 2>&1 || true
  fi
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

cat >"$root/pane.sh" <<'PANE'
# One demo pane: wait for the go file, then play a timeline of titles and status reports.
seat=$1 go=$2
title() { printf '\033]2;%s\007' "$1"; }
report() { # state, message, [kind]
  local kind=""
  [ -n "${3:-}" ] && kind=":kind=$3"
  printf '\033]7501;state=%s%s:app=claude-code:msg=%s\007' "$1" "$kind" "$(printf '%s' "$2" | base64)"
}
# at N: wait until N seconds after the go file appears.
now=0
at() { while [ ! -e "$go" ]; do sleep 0.1; done; sleep $(($1 - now)); now=$1; }
case "$seat" in
  seat-a)
    title "⠋ seat-a@demo"; printf '$ cargo test --workspace\n   Compiling parser v0.3.0\n'
    at 0; report working "Running the test suite"
    at 12; printf 'running 12 tests\ntest parser::nested ... ok\n'; report blocked "Allow Bash: cargo test --workspace" permission
    at 26; title "⠋ seat-a@demo"; report working "Fixing the nested parser case"
    at 36; title "✳ seat-a@demo"; printf 'test result: ok. 12 passed\n'; report done "Tests pass: 12 passed" ;;
  seat-b)
    title "⠋ seat-b@demo"; printf 'Reading src/parser.rs\nReading tests/parser.rs\n'
    at 0; report working "Reviewing the parser change"
    at 18; title "✳ seat-b@demo"; printf 'Posted 2 comments\n'; report done "Review posted with 2 comments" ;;
  seat-c)
    title "✳ seat-c@demo"; printf 'Edit src/app.tsx\n  - import { Board } from "./board";\n  + import { Board, Row } from "./board";\n'
    report blocked "Allow Edit: src/app.tsx" permission ;;
  seat-d)
    title "✳ seat-d@demo"; printf 'Docs updated.\n'; report idle "Waiting for input" ;;
  seat-e)
    title "✳ seat-e@demo"; printf 'Ready.\n' ;;
esac
exec sleep 600
PANE

go="$root/go"
rex new api --window build -- sh "$root/pane.sh" seat-a "$go" >/dev/null
rex run -s api --focus=false -- sh "$root/pane.sh" seat-b "$go" >/dev/null
rex new web --window deploy -- sh "$root/pane.sh" seat-c "$go" >/dev/null
rex run -s web --focus=false -- sh "$root/pane.sh" seat-d "$go" >/dev/null
rex run -s web --focus=false -- sh "$root/pane.sh" seat-e "$go" >/dev/null
sleep 1
# Pane and tab labels, by the seat each pane titles itself with.
rex do - >/dev/null <<'LUA'
local tabs = { ["seat-a"] = "build", ["seat-b"] = "review", ["seat-c"] = "deploy",
  ["seat-d"] = "docs", ["seat-e"] = "notes" }
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
sleep 4; k 1; sleep 1; k Space; sleep 4; k Space
sleep 3; k 2; sleep 1; k s; sleep 3
k '?'; sleep 3.5; k '?'
sleep 2; k / r e v i e w Enter; sleep 3.5; k Escape
sleep 13; k S; sleep 4
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
agg --theme "$theme" --font-size 14 --idle-time-limit 2 --fps-cap 15 --last-frame-duration 3 \
  "$repo/docs/assets/demo.cast" "$repo/docs/assets/demo.gif" >/dev/null 2>&1
echo "wrote docs/assets/demo.cast and docs/assets/demo.gif"
