#!/usr/bin/env bash
# Records the README demo, docs/assets/demo.cast and demo.gif: the board on a private,
# throwaway Rex server whose panes play a scripted day, driven with tmux keys.
#
# Nothing here touches your own Rex server or rexboard: Rex, rexboard serve and the board run
# under a temporary HOME and XDG dirs, and every one of them is stopped at the end. The
# clipboard is a file in that temporary dir, so a copy never reaches yours.
#
# REXBOARD_DEMO_DENY may name a file of strings (one per line) that must not appear in the cast,
# such as your real host, session and tab labels. /Users/, /home/, /tmp/, /private/, your user
# name, this machine's name and your home path are always refused.
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$here/.." && pwd)
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
# Refused in the cast, besides the fixed list and REXBOARD_DEMO_DENY.
denied=$(printf '%s\n' "$(id -un)" "$(hostname -s)" "$HOME")
# Dracula-like colours, with palette 8 (the board's selected row) a quiet raise over the page.
theme=282a36,f8f8f2,21222c,ff5555,50fa7b,f1fa8c,8be9fd,ff79c6,8be9fd,f8f8f2,3a3f52,ff6e6e,69ff94,ffffa5,a4ffff,ff92df,a4ffff,ffffff
cols=140 rows=30
title="rexboard: an attention board for agent panes across Rex hosts"

# A short root, since unix socket paths are limited to about 100 bytes.
root=$(mktemp -d /tmp/rbdemo.XXXX)
mkdir -p "$root/home/.config" "$root/run" "$root/state" "$root/data" "$root/cache" "$root/tmp" "$root/bin"
chmod 700 "$root/run"
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

# A demo extension: a PROJECT column (the session label up to the `-`), a hidden seat field, and
# a send action whose program only writes the message to a file here and says it was sent.
ext="$XDG_CONFIG_HOME/rexboard/extensions/team"
mkdir -p "$ext"
cat >"$ext/extension.toml" <<'TOML'
name = "team"
contract = 1
description = "Each row's project, and a message to the seat in it"
cmd = ["./adapter"]
every = 3
timeout = 2

[[fields]]
name = "project"
title = "PROJECT"

[[fields]]
name = "seat"
title = "SEAT"
hidden = true

[[actions]]
name = "send"
title = "Send message"
key = "s"
cmd = ["./send", "--", "{field:seat}", "{text}"]
confirm = "send to {field:seat}"
when = ["seat"]
TOML
cat >"$ext/adapter" <<'ADAPTER'
#!/bin/sh
exec jq -c '{contract: 1, rows: (.rows | map(select((.pane // "") | test("^seat-")) | ((.session // "") | split("-")[0]) as $p | {key, value: {fields: {project: $p, seat: "\(.pane)@\($p)"}}}) | from_entries)}'
ADAPTER
printf '#!/bin/sh\nshift\nprintf "%%s: %%s\\n" "$1" "$2" >>"%s/sent"\necho "Sent to $1"\n' "$root" >"$ext/send"
chmod +x "$ext/adapter" "$ext/send"
printf '[extensions]\nenabled = ["team"]\n' >"$XDG_CONFIG_HOME/rexboard/config.toml"
# A clipboard that is a file here, whichever tool the board picks.
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

# Start serve now, so the cast opens on the board, not on `connecting to serve`.
"$board" ls >/dev/null 2>&1 || true

cast="$root/demo.cast"
tmux -L "$tmux_socket" new-session -d -x "$cols" -y "$rows" -s demo \
  "asciinema rec '$cast' --overwrite --quiet --window-size ${cols}x${rows} --idle-time-limit 2 \
   --title \"$title\" --command '$board board'"
tmux -L "$tmux_socket" set-option -t demo status off >/dev/null
k() { local key; for key in "$@"; do tmux -L "$tmux_socket" send-keys -t demo "$key"; sleep 0.35; done; }
# lit TEXT: typed into an open input line, a character at a time.
lit() { local i; for ((i = 0; i < ${#1}; i++)); do tmux -L "$tmux_socket" send-keys -t demo -l -- "${1:i:1}"; sleep 0.07; done; }
for _ in $(seq 1 100); do
  tmux -L "$tmux_socket" capture-pane -p -t demo 2>/dev/null | grep -qF 'NEEDS YOU' && break
  sleep 0.1
done
tmux -L "$tmux_socket" capture-pane -p -t demo | grep -qF 'NEEDS YOU' || { echo "the board never showed NEEDS YOU" >&2; exit 1; }
sleep 1.5
touch "$go"
# The spinners and every shape; a done row's card, then `m` marks it seen; the permission row's
# ask, `a` on its own, `2` once to show that a lasting option asks again, then `1` answers it
# from the board; `z` shows what may follow it and `za` folds a group and opens it again;
# `/docs` narrows the list to the docs row, whose live, coloured full view follows its diff; `q`
# back as it finishes, then `dd` asks before `y` closes it. Every opener (`a`, the first `d`)
# comes after a pause, as the typing guard asks.
sleep 2.5; k 3; sleep 2.2
k m; sleep 1.2
k 2; sleep 2
k a; sleep 3; k 2; sleep 2; k 1; sleep 2.5
k z; sleep 1.5; k a; sleep 1.2; k z a; sleep 0.8
k / d o c s Enter; sleep 1.5; k Space; sleep 4; k q; sleep 1.2
k d d; sleep 3; k y; sleep 1.8; k Escape; sleep 1.5
# `x s` on the review row: type a line, read the red confirm, `y` sends it through the extension.
k / s e a t - b Enter; sleep 1.2; k x; sleep 1.2; k s; sleep 0.8
lit "thanks, merging now"; sleep 0.6; k Enter; sleep 2.2; k y; sleep 2.5; k Escape; sleep 1
k q
for _ in $(seq 1 300); do tmux -L "$tmux_socket" has-session -t demo 2>/dev/null || break; sleep 0.1; done

python3 - "$cast" "${REXBOARD_DEMO_DENY:-}" "$denied" <<'PY'
import json, sys
path, deny_file, denied = sys.argv[1:4]
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
deny += [line.strip() for line in denied.splitlines() if line.strip()]
if deny_file:
    deny += [line.strip() for line in open(deny_file) if line.strip()]
found = sorted({token for token in deny if token in blob})
if found:
    sys.exit(f"private data in the cast: {len(found)} denied string(s); not writing it")
open(path, "w").write(blob + "\n")
PY
cp "$cast" "$repo/docs/assets/demo.cast"
agg --theme "$theme" --font-size 12 --idle-time-limit 2 --fps-cap 8 --last-frame-duration 3 \
  "$repo/docs/assets/demo.cast" "$repo/docs/assets/demo.gif" >/dev/null 2>&1
echo "wrote docs/assets/demo.cast and .gif"
