# System architecture

rexboard reads Rex, remembers what Rex does not, and shows it. Nothing writes to Rex.

```
 Rex host (local)        Rex host (mini)
       │                        │
 rex do - --all          rex -S mini do - --all       one observer per host,
 observer.lua            observer.lua                 NDJSON lines (docs/cli.md)
       └───────────┬────────────┘
                   ▼
            rexboard serve        collectors → one reducer thread (Board)
            │      │      │       state.json, transitions.jsonl, serve.log
            ▼      ▼      ▼
           ls   watch   seen      clients over a unix socket
```

## Pieces

| Module | Does |
| --- | --- |
| `src/lua/observer.lua` | Runs inside `rex do - --all --fail-fast`: one snapshot, then one line per event, and a `tick` every 30s |
| `src/collector.rs` | Keeps one observer alive per host, with backoff, in its own process group |
| `src/event.rs` | The observer line contract, `v` 1 |
| `src/board.rs` | The reducer: rows with `since`, turns, `seen` and groups, from host events and an injected clock |
| `src/serve.rs` | The daemon: collectors, the reducer thread, the socket |
| `src/state.rs` | Paths, the `flock`, `state.json`, `transitions.jsonl`, and the startup sweep |
| `src/client.rs` | `ls`, `watch --json`, `seen` and `serve` as clients; starts serve when nothing answers |
| `src/rex.rs` | Finds `rex`, runs one-shot scripts, and holds the one lock around every spawn, bind, accept and connect |

serve uses std threads and channels; there is no async runtime. Collectors, the socket listener and each connection run on their own thread, and only the reducer thread touches the board.

## Files

| Path | What |
| --- | --- |
| `$XDG_RUNTIME_DIR/rexboard.sock`, else `$TMPDIR/rexboard-UID.sock` (`/tmp` when `TMPDIR` is unset) | The socket, mode 0600. A path over 103 bytes falls back to `$TMPDIR`, then fails with `SERVE`. A client refuses a socket served by another uid |
| `$XDG_STATE_HOME/rexboard/` (default `~/.local/state/rexboard/`) | The state dir, mode 0700 |
| `serve.lock` | The `flock` that keeps serve to one instance, plus `{pid, rex, children}`: serve's pid, the `rex` path and its observers' pids |
| `state.json` | `{"v":1,"panes":{KEY:{state,kind,since,since_estimated,turn_started,last_turn,seen}}}`, written through a temp file and a rename at most once a second, and on stop. Entries for a host that has not sent its first snapshot this run are kept as loaded |
| `transitions.jsonl` | One line per state change: `{at, host, block_id, from, to, kind}`. Over 5 MB at serve start, it moves to `transitions.jsonl.1`, replacing the older one |
| `serve.log` | serve's output when a command started it. A client clears it first when it is over 1 MB |

On start, serve takes the lock, removes a stale socket, then stops observers a dead serve left behind. A recorded pid is stopped only if it is still its own process group, its parent is init or the recorded serve, and its command is the recorded `rex` running `do - --all --fail-fast`. Anything else is skipped and logged, because the pid may now belong to another program.

## Protocol

One JSON request per line on the socket; every reply line carries `"v": 1`.

| Request | Reply |
| --- | --- |
| `{"cmd":"rows"}` | `{"v":1,"rows":[ROW...],"hosts":{HOST:STATE}}`. `STATE` is `starting`, `up`, `down` or `stopped`. Right after serve starts, the reply waits up to 5s for every host's first snapshot |
| `{"cmd":"subscribe"}` | The same `rows` line now and on every change (at most every 100ms), plus `{"v":1,"heartbeat":true}` every second. Each subscriber has room for 4 lines; a reader that falls behind misses lines rather than growing serve, and the next `rows` line is a full snapshot again |
| `{"cmd":"seen","key":"HOST/BLOCK_ID"}` | `{"v":1,"ok":true,"key":KEY}` |
| `{"cmd":"next","after":KEY,"back":false}` | `{"v":1,"row":ROW}`: the next `needs_you` row after `after` (a key or a block id; else the first), unseen rows first and all of them once every row is seen, wrapping; `back` goes the other way |
| `{"cmd":"stop"}` | `{"v":1,"ok":true}`, then serve saves and exits |

Errors are `{"v":1,"error":{"code":CODE,"message":...}}`, with `CODE` one of `USAGE`, `NOT_FOUND` and `NOBODY_NEEDS_YOU`. `ROW` is the object `ls --json` prints.
