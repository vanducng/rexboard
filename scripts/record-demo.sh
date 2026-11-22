#!/usr/bin/env bash
# Records the README demo, docs/assets/demo.cast and demo.gif: the board over three private,
# throwaway Rex hosts, this one, mini and studio, driven with tmux keys.
#
# Nothing here touches your own Rex server or rexboard: three Rex servers (this host, mini and
# studio), rexboard serve and the board run under a temporary HOME and XDG dirs, every one of
# them is proved to live under that dir before it is used, and every one is stopped at the end.
# The hosts are unix sockets under the temp dir, so no network is involved. The clipboard is a
# file in that dir, so a copy never reaches yours.
#
# REXBOARD_DEMO_DENY may name a file of strings (one per line) that must not appear in the cast,
# such as your real host, session and tab labels. /Users/, /home/, /tmp/, /private/, your user
# name, this machine's name and your home path are always refused.
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$here/.." && pwd)
if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
for tool in rex asciinema agg tmux python3 cargo base64; do
  command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
# The tools are found now, under the real HOME: a version manager's shim would read that HOME's
# config, which the temporary one below hides, so a shim is swapped for the tool it stands for.
real() {
  local path
  path=$(command -v "$1")
  if [[ "$(basename -- "$(readlink -f -- "$path")")" == mise ]]; then
    mise which "$1" 2>/dev/null || echo "$path"
  else
    echo "$path"
  fi
}
tmux=$(real tmux)
py=$(real python3)
demo_sock=zzdemo
rec_sock=zzrec
for sock in "$demo_sock" "$rec_sock"; do
  "$tmux" -L "$sock" has-session 2>/dev/null && { echo "tmux server $sock is already running" >&2; exit 1; }
done
(cd "$repo" && cargo build --quiet)
board="${CARGO_TARGET_DIR:-$repo/target}/debug/rexboard"
# Refused in the cast, besides the fixed list and REXBOARD_DEMO_DENY.
denied=$(printf '%s\n' "$(id -un)" "$(hostname -s)" "$HOME")
# Dracula-like colours, with palette 8 (the board's selected row) a quiet raise over the page.
theme=282a36,f8f8f2,21222c,ff5555,50fa7b,f1fa8c,8be9fd,ff79c6,8be9fd,f8f8f2,3a3f52,ff6e6e,69ff94,ffffa5,a4ffff,ff92df,a4ffff,ffffff
# The board has 30 rows; a status line under it carries the caption of each scene.
cols=140 rows=31
title="rexboard: an attention board for agent panes across Rex hosts"

# A short root, since unix socket paths are limited to about 100 bytes.
root=$(mktemp -d /tmp/rbdemo.XXXX)
chmod 700 "$root"
# `as NAME CMD...` runs CMD with the HOME and XDG dirs of the server NAME, under $root.
as() {
  local d=$root/$1
  shift
  (
    export HOME="$d/home" XDG_RUNTIME_DIR="$d/run" XDG_CONFIG_HOME="$d/home/.config" \
      XDG_STATE_HOME="$d/state" XDG_DATA_HOME="$d/data" XDG_CACHE_HOME="$d/cache" TMPDIR="$d/tmp"
    "$@"
  )
}
for name in local mini studio; do
  mkdir -p "$root/$name"/{home/.config,run,state,data,cache,tmp}
  chmod 700 "$root/$name/run"
done
mkdir -p "$root/bin"
# This shell is the local server's: the board, serve and every rex call here use its dirs.
export HOME="$root/local/home" XDG_RUNTIME_DIR="$root/local/run" XDG_CONFIG_HOME="$root/local/home/.config" \
  XDG_STATE_HOME="$root/local/state" XDG_DATA_HOME="$root/local/data" XDG_CACHE_HOME="$root/local/cache" \
  TMPDIR="$root/local/tmp" PATH="$root/bin:$PATH"
unset REX_SERVER REX_SESSION REX_BLOCK REX_CLIENT REX_HOME REXBOARD_ICONS REXBOARD_REDUCED_MOTION
# NAME:PID of each server proved to be ours; nothing else is ever stopped.
ours=()
is_ours() { ps -o command= -p "$1" 2>/dev/null | grep -qF "$root/"; }
cleanup() {
  "$tmux" -L "$rec_sock" kill-server 2>/dev/null || true
  "$tmux" -L "$demo_sock" kill-server 2>/dev/null || true
  # The private serve, whose socket lives under $root/local/state; XDG_STATE_HOME keeps --stop there.
  "$board" serve --stop >/dev/null 2>&1 || true
  local entry pid
  for entry in ${ours[@]+"${ours[@]}"}; do
    pid=${entry#*:}
    if is_ours "$pid"; then
      kill "$pid" 2>/dev/null || true
      # It writes a last snapshot as it exits, so the dir goes only once it has.
      for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    fi
  done
  rm -rf "$root"
}
trap cleanup EXIT

# Start the private server NAME, then prove it is ours: its socket and its process both live
# under $root/NAME. Anything else is someone else's server, so refuse without touching it.
start_server() {
  local name=$1 endpoint pid
  as "$name" rex server start >/dev/null 2>&1 || true
  read -r endpoint pid < <(as "$name" rex server status --json 2>/dev/null |
    "$py" -c 'import json, sys; d = json.load(sys.stdin); print(d.get("endpoint", "-"), d.get("pid", "-"))' ||
    echo "- -")
  if [[ "$endpoint" != "unix://$root/$name/"* ]] || ! is_ours "$pid"; then
    echo "the Rex server rex reaches for $name is not the private one under $root/$name; refusing to record" >&2
    exit 1
  fi
  ours+=("$name:$pid")
}
start_server local
start_server mini
start_server studio
for name in mini studio; do
  rex hosts add "$name" "unix://$root/$name/run/rex/server.sock" >/dev/null
done
# A second gate: this server's hosts are exactly mini and studio, each a socket under $root, and
# its home is this machine.
rex hosts --json | "$py" -c '
import json, sys
root = sys.argv[1]
d = json.load(sys.stdin)
labels = sorted(h["host"]["label"] for h in d.get("hosts") or [])
ends = [e for h in d.get("hosts") or [] for e in h["host"].get("endpoints") or []]
ok = (
    labels == ["mini", "studio"]
    and d.get("home", {}).get("this_machine") is True
    and all(e.startswith("unix://" + root + "/") for e in ends)
)
sys.exit(0 if ok else 1)
' "$root" || { echo "the demo Rex server's hosts are not exactly mini and studio under $root; refusing to record" >&2; exit 1; }

# Model aliases: the board shows each harness's model id in one short style. No row reports a
# five-hour quota, so 5H stays hidden, as every column does while no row has a value.
mkdir -p "$XDG_CONFIG_HOME/rexboard"
cat >"$XDG_CONFIG_HOME/rexboard/config.toml" <<'TOML'
[models]
"claude-opus-4-5" = "opus-4.5"
"claude-sonnet-4-5" = "sonnet-4.5"
"gpt-5.1-codex" = "5.1-codex"
TOML
# A clipboard that is a file here, whichever tool the board picks.
for copy in pbcopy wl-copy xclip; do
  printf '#!/bin/sh\ncat >"%s/clipboard"\n' "$root" >"$root/bin/$copy"
  chmod +x "$root/bin/$copy"
done

# A pane counts as a harness's while its foreground process is named like one, so the demo panes
# run under copies of bash named like the four harnesses' processes.
grok=grok-1.0.0-darwin-arm64
for name in claude codex pi "$grok"; do cp /bin/bash "$root/bin/$name"; done
cat >"$root/pane.sh" <<'PANE'
# One demo pane: its title, a screen, a usage record and a status report, then held open.
seat=$1 go=$2
# Each harness reports under its own app name.
case "$seat" in
  seat-b | seat-e | seat-j | seat-m) app=codex ;;
  seat-i) app=grok-build ;;
  seat-g) app=pi ;;
  *) app=claude-code ;;
esac
title() { printf '\033]2;%s\007' "$1"; }
report() { # state, message (lines joined with " ⏎ "), [kind]
  local kind=""
  [ -n "${3:-}" ] && kind=":kind=$3"
  printf '\033]7501;state=%s%s:app=%s:msg=%s\007' "$1" "$kind" "$app" "$(printf '%s' "$2" | base64)"
}
# usage JSON: the model and context the pane's harness would report, as a usage record.
usage() { printf '\033]7501;id=usage:app=%s:state=done:msg=%s\007' "$app" "$(printf '%s' "$1" | base64)"; }
case "$seat" in
  seat-a)
    title "✳ seat-a@demo"; printf '● Bash(cargo test --workspace --features full)\n\n Do you want to proceed?\n ❯ 1. Yes\n'
    usage '{"v":1,"m":"claude-opus-4-5","e":"high","c":38,"w":200000,"t":76000,"u":0.84}'
    report blocked "Bash wants to run: cargo test --workspace --features full" permission ;;
  seat-b)
    title "✳ seat-b@demo"; printf '● Posted 2 comments on the parser change.\n'
    usage '{"v":1,"m":"gpt-5.1-codex","e":"xhigh","c":61,"w":400000,"t":244000}'
    report done "Review posted with 2 comments ⏎ Both are about naming in src/parser.rs; nothing blocks the merge." ;;
  seat-c)
    title "✳ seat-c@demo"; printf 'Which test runner should the web package use?\n\n❯ 1. vitest\n  2. jest\n'
    usage '{"v":1,"m":"claude-sonnet-4-5","e":"medium","c":17,"w":200000,"t":34000,"u":0.12}'
    report blocked "Which test runner should the web package use? ⏎ 1. vitest: fast, native ESM ⏎ 2. jest: familiar, more plugins" question ;;
  seat-d)
    title "⠋ seat-d@demo"; printf '● Updating the style docs.\n'
    usage '{"v":1,"m":"claude-opus-4-5","e":"high","c":54,"w":200000,"t":108000,"u":1.31}'
    report working "Updating the style docs" ;;
  seat-e)
    title "✳ seat-e@demo"; printf 'Ready.\n'
    usage '{"v":1,"m":"gpt-5.1-codex","e":"high","c":9,"w":400000,"t":36000}'
    report idle "Waiting for input" ;;
  seat-f)
    title "⠋ seat-f@demo"; printf '● Fixing the nested parser case.\n'
    usage '{"v":1,"m":"claude-opus-4-5","e":"high","c":44,"w":200000,"t":88000,"u":0.97}'
    report working "Fixing the nested parser case" ;;
  seat-g)
    title "✳ seat-g@demo"; printf 'error: 2 errors in the release build\n'
    usage '{"v":1,"m":"claude-sonnet-4-5","e":"low","c":72,"w":200000,"t":144000,"u":0.58}'
    report error "Build failed: 2 errors in the release build" ;;
  seat-h)
    title "✳ seat-h@demo"; printf '● Updated the guides.\n'
    usage '{"v":1,"m":"claude-sonnet-4-5","e":"medium","c":22,"w":200000,"t":44000,"u":0.2}'
    report done "Guides updated ⏎ The install and config pages now match the code." ;;
  seat-i)
    title "✳ seat-i@demo"; printf 'Waiting for input.\n'
    report idle "Waiting for input" ;;
  seat-j)
    title "⠋ seat-j@demo"; printf '● Running the integration suite.\n'
    usage '{"v":1,"m":"gpt-5.1-codex","e":"high","c":47,"w":400000,"t":188000}'
    report working "Running the integration suite" ;;
  seat-l)
    title "⠋ seat-l@demo"; printf '● Rotating the logs.\n'
    usage '{"v":1,"m":"claude-sonnet-4-5","e":"low","c":31,"w":200000,"t":62000,"u":0.27}'
    report working "Rotating the logs" ;;
  seat-m)
    title "✳ seat-m@demo"; printf 'Ready.\n'
    usage '{"v":1,"m":"gpt-5.1-codex","e":"medium","c":13,"w":400000,"t":52000}'
    report idle "Waiting for input" ;;
  seat-k)
    title "✳ seat-k@demo"; printf '● Release notes drafted.\n'
    usage '{"v":1,"m":"claude-opus-4-5","e":"medium","c":28,"w":200000,"t":56000,"u":0.49}'
    report done "Release notes drafted ⏎ Three user-facing changes, one fix." ;;
esac
# Hold the pane with a builtin, so the harness's own process stays in front.
mkfifo "$go.$seat"
read -r -t 900 <>"$go.$seat" || :
PANE

go="$root/go"
# pane HOST HARNESS SEAT REX-ARGS...: one demo pane on that host's server, under that harness's name.
pane() {
  local host=$1 bin=$2 seat=$3
  shift 3
  as "$host" rex "$@" -- "$root/bin/$bin" "$root/pane.sh" "$seat" "$go" >/dev/null
}
pane studio claude seat-a new api --window build
pane studio codex seat-b run -s api --focus=false
pane studio claude seat-c new web --window deploy
pane studio claude seat-d run -s web --focus=false
pane studio codex seat-e run -s web --focus=false
pane mini claude seat-f new api --window build
pane mini pi seat-g run -s api --focus=false
pane mini claude seat-h new docs --window guides
pane mini "$grok" seat-i run -s docs --focus=false
pane mini codex seat-j new web --window deploy
pane mini claude seat-k run -s web --focus=false
pane local claude seat-l new ops --window monitor
pane local codex seat-m run -s ops --focus=false
sleep 1
# Tab labels, by the seat each pane titles itself with, on each server.
label_tabs() {
  as "$1" rex do - >/dev/null <<'LUA'
local tabs = { ["seat-a"] = "build", ["seat-b"] = "review", ["seat-c"] = "deploy", ["seat-d"] = "docs",
  ["seat-e"] = "notes", ["seat-f"] = "build", ["seat-g"] = "infra", ["seat-h"] = "guides",
  ["seat-i"] = "notes", ["seat-j"] = "deploy", ["seat-k"] = "release",
  ["seat-l"] = "monitor", ["seat-m"] = "backups" }
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
}
label_tabs local
label_tabs mini
label_tabs studio

# Start serve now, and wait for every host's rows, so the cast opens on the full board.
"$board" ls >/dev/null 2>&1 || true
for _ in $(seq 1 100); do
  n=$("$board" ls --json 2>/dev/null | "$py" -c 'import json, sys; print(len(json.load(sys.stdin)))' 2>/dev/null || echo 0)
  [[ "$n" == 13 ]] && break
  sleep 0.2
done
[[ "$n" == 13 ]] || { echo "the board has $n rows, not 13" >&2; exit 1; }

cast="$root/demo.cast"
# The board runs in one tmux session whose status line carries the scene captions; asciinema
# records a second tmux that attaches to it, so the caption is on screen with the board.
"$tmux" -L "$demo_sock" new-session -d -x "$cols" -y "$rows" -s demo "$board board"
dset() { "$tmux" -L "$demo_sock" set-option -t demo "$@" >/dev/null; }
dset status on
dset status-position bottom
dset status-left-length 140
dset status-right ''
dset status-style 'bg=#21222c,fg=#f1fa8c'
dset window-status-format ''
dset window-status-current-format ''
"$tmux" -L "$rec_sock" new-session -d -x "$cols" -y "$rows" -s rec \
  "asciinema rec '$cast' --overwrite --quiet --window-size ${cols}x${rows} --idle-time-limit 2 \
   --title \"$title\" --command '$tmux -L $demo_sock attach -t demo'"
"$tmux" -L "$rec_sock" set-option -t rec status off >/dev/null
# cap TEXT: the caption of the scene, on the status line under the board.
cap() { dset status-left "#[bold] $1"; }
k() { local key; for key in "$@"; do "$tmux" -L "$demo_sock" send-keys -t demo "$key"; sleep 0.45; done; }
# lit TEXT: typed into an open input line, a character at a time.
# tmux reads a lone `;` argument as a command separator, so it goes as `\;`.
lit() {
  local i c
  for ((i = 0; i < ${#1}; i++)); do
    c=${1:i:1}; [[ $c == ";" ]] && c='\;'
    "$tmux" -L "$demo_sock" send-keys -t demo -l -- "$c"; sleep 0.09
  done
}
for _ in $(seq 1 100); do
  "$tmux" -L "$rec_sock" capture-pane -p -t rec 2>/dev/null | grep -qF 'NEEDS YOU' && break
  sleep 0.1
done
"$tmux" -L "$rec_sock" capture-pane -p -t rec | grep -qF 'NEEDS YOU' || { echo "the board never showed NEEDS YOU" >&2; exit 1; }
touch "$go"

# Seven scenes, each with a caption. Digits select the row with that number, as `1`-`99` do by
# default. There is no jump scene: a jump needs a Rex app, and a private server has none.
cap "1/7  The board across three hosts: what needs you is on top, whichever host runs it"
sleep 9
cap "2/7  v steps through the views, :se opens the session view"
k v; sleep 2.2; k v; sleep 2; k :; lit "se"; sleep 0.5; k Enter; sleep 3.5
cap "3/7  / narrows the list by text, esc clears it"
k /; lit "api"; k Enter; sleep 4.5; k Escape; sleep 2.5
cap "4/7  f and a letter shows one status: p permission, d done"
k f p; sleep 4; k f f; sleep 1.2; k f d; sleep 4; k f f; sleep 2
cap "5/7  * marks a pane as a favorite, f* shows only the marked rows"
k 4 '*'; sleep 1.5; k 11 '*'; sleep 1.8; k f '*'; sleep 4.5; k f f; sleep 2
cap "6/7  + keeps a group, f+ shows only the kept ones, / narrows inside them, esc keeps them"
k 1 '+'; sleep 1.5; k 6 '+'; sleep 1.8; k f '+'; sleep 4; k /; lit "claude"; k Enter; sleep 4; k Escape; sleep 3.5; k f '+'; sleep 2.5
cap "7/7  n walks the rows that need you, a digit picks a row"
k n; sleep 3; k n; sleep 3; k 3; sleep 3
"$tmux" -L "$demo_sock" kill-server
for _ in $(seq 1 300); do "$tmux" -L "$rec_sock" has-session -t rec 2>/dev/null || break; sleep 0.1; done

"$py" - "$cast" "${REXBOARD_DEMO_DENY:-}" "$denied" <<'PY'
import json, re, sys
path, deny_file, denied = sys.argv[1:4]
lines = open(path).read().splitlines()
header = json.loads(lines[0])
header["command"] = "rexboard board"
header.pop("env", None)
header.get("term", {}).pop("type", None)
lines[0] = json.dumps(header)
# The attached tmux asks the terminal about itself (device status, attributes, colours); a
# replay has nobody to answer, and an emulator reading the cast would choke on them.
queries = re.compile(r"\x1b\[\?996n|\x1b\[>?c|\x1b\[>q|\x1b\]1[01];\?\x1b\\")
for i, line in enumerate(lines[1:], 1):
    if line.strip():
        event = json.loads(line)
        event[2] = queries.sub("", event[2])
        lines[i] = json.dumps(event, ensure_ascii=False)
# End on the board, not on the screen it leaves behind when the recording ends.
for i, line in enumerate(lines[1:], 1):
    if line.strip() and "\x1b[?1049l" in json.loads(line)[2]:
        lines = lines[:i]
        break
blob = "\n".join(lines)
deny = ["/Users/", "/home/", "/tmp/", "/private/", "claude.ai", ".ts.net", "mise ERROR"]
deny += [line.strip() for line in denied.splitlines() if line.strip()]
if deny_file:
    deny += [line.strip() for line in open(deny_file) if line.strip()]
found = sorted({token for token in deny if token in blob})
if found:
    sys.exit(f"private data in the cast: {len(found)} denied string(s); not writing it")
open(path, "w").write(blob + "\n")
PY
cp "$cast" "$repo/docs/assets/demo.cast"
agg --theme "$theme" --font-size 14 --idle-time-limit 2 --fps-cap 8 --last-frame-duration 3 \
  "$repo/docs/assets/demo.cast" "$repo/docs/assets/demo.gif" >/dev/null 2>&1
echo "wrote docs/assets/demo.cast and .gif"
