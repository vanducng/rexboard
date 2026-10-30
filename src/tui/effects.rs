//! What the board's effects do, off its loop: reads of panes, jumps, closes, answers, seen, and
//! the messages that come back to the board, and serve's rows as they change.

use super::*;

/// Peek keeps this many lines from the end of the pane, enough for a 200-row pane; each draw shows
/// the newest that fit, so a resize needs no new capture.
const PEEK_LINES: usize = 200;

/// A peek reads at most this much of the end of a pane, and gives up after PEEK_WITHIN.
const PEEK_BYTES: usize = 48 * 1024;

/// The same end with its colours: about 2x the bytes per line (2.0x on a Claude Code pane, 2.5x
/// on a coloured diff), so PEEK_LINES still fit at up to about 490 bytes a line.
const PEEK_VT_BYTES: usize = 96 * 1024;

const PEEK_WITHIN: Duration = Duration::from_secs(3);

/// Each peek takes the next number; one whose number is no longer the latest stops.
static PEEK_GEN: AtomicU64 = AtomicU64::new(0);

pub(super) enum Msg {
    Rows(Rows),
    Gone(String),
    Peek(String, std::result::Result<Vec<String>, String>),
    Say(String),
    /// A failure the footer shows in the alert role until the next key.
    Failed(String),
    Went(std::result::Result<String, String>),
    Screen(String, std::result::Result<Screen, String>),
    Sent {
        key: String,
        what: String,
        result: std::result::Result<(), String>,
    },
}

/// A message from a thread; a refused answer asks for the pane to be read again.
pub(super) fn apply(app: &mut App, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Rows(rows) => {
            app.set_extensions(rows.fields, rows.extensions);
            app.offered = rows.actions;
            app.set_rows(rows.rows, rows.hosts);
        }
        Msg::Gone(why) => {
            app.say(format!("{why} · R reconnects"));
            app.link = Link::Lost(why);
        }
        Msg::Peek(key, lines) => app.peeked(&key, lines),
        Msg::Say(text) => app.say(text),
        Msg::Failed(why) => app.failed = Some(why),
        Msg::Went(result) => app.went(result),
        Msg::Screen(key, screen) => app.screen(&key, screen),
        Msg::Sent { key, what, result } => return app.sent(&key, &what, result),
    }
    None
}

pub(super) fn act(effect: Effect, tx: &Sender<Msg>, generation: &Arc<AtomicU64>) {
    let tx = tx.clone();
    match effect {
        Effect::Seen { key, label, state } => {
            thread::spawn(move || {
                let text = match client::call(&json!({ "cmd": "seen", "key": key })) {
                    Ok(_) => seen_message(&label, &state),
                    Err(err) => format!("seen failed: {err}"),
                };
                let _ = tx.send(Msg::Say(text));
            });
        }
        Effect::SeenAll(keys) => {
            thread::spawn(move || {
                let mut marked = 0;
                let mut failed = None;
                for key in keys {
                    match client::call(&json!({ "cmd": "seen", "key": key })) {
                        Ok(_) => marked += 1,
                        Err(err) => failed = Some(err),
                    }
                }
                let rows = if marked == 1 { "row" } else { "rows" };
                let text = match failed {
                    None => format!("{marked} done {rows} marked seen"),
                    Some(err) => format!("{marked} done {rows} marked seen; seen failed: {err}"),
                };
                let _ = tx.send(Msg::Say(text));
            });
        }
        Effect::Jump(row) => {
            thread::spawn(move || {
                let result = jump::go(&row)
                    .map(|()| format!("jumped to {}", path(&labels(&row, true), usize::MAX)))
                    .map_err(|err| err.to_string());
                let _ = tx.send(Msg::Went(result));
            });
        }
        Effect::Back => {
            thread::spawn(move || {
                let result = jump::previous()
                    .and_then(|row| jump::go(&row).map(|()| row))
                    .map(|row| format!("back to {}", path(&labels(&row, true), usize::MAX)))
                    .map_err(|err| err.to_string());
                let _ = tx.send(Msg::Went(result));
            });
        }
        Effect::Close(row) => {
            thread::spawn(move || {
                let place = path(&labels(&row, false), usize::MAX);
                let result = rex::locate()
                    .map_err(|err| format!("close failed: {err}"))
                    .and_then(|rex| send::close(&rex, &row.pane))
                    .map(|()| format!("closed {place} on {}", row.pane.host));
                let _ = tx.send(Msg::Went(result));
            });
        }
        Effect::Run { plan, title } => {
            thread::spawn(move || {
                let result = crate::extension::action::run(&plan)
                    .map(|out| format!("{title}: {}", if out.is_empty() { "done" } else { &out }))
                    .map_err(|why| format!("{title} failed: {why}"));
                let _ = tx.send(Msg::Went(result));
            });
        }
        Effect::Copy { text, shown } => {
            thread::spawn(move || {
                let _ = tx.send(match clipboard::copy(&text) {
                    Ok(()) => Msg::Say(format!("copied {shown}")),
                    Err(why) => Msg::Failed(format!("could not copy: {why}")),
                });
            });
        }
        Effect::Peek {
            key,
            host,
            session,
            block,
            vt,
        } => {
            let mine = PEEK_GEN.fetch_add(1, Ordering::SeqCst) + 1;
            thread::spawn(move || {
                let current = || PEEK_GEN.load(Ordering::SeqCst) == mine;
                let lines = match rex::locate() {
                    Ok(rex) => peek(&rex, &host, &session, &block, vt, &|| !current()),
                    Err(err) => Some(Err(format!("peek failed: {err}"))),
                };
                if let Some(lines) = lines.filter(|_| current()) {
                    let _ = tx.send(Msg::Peek(key, lines));
                }
            });
        }
        Effect::Read {
            key,
            host,
            session,
            block,
        } => {
            thread::spawn(move || {
                let screen = rex::locate()
                    .map_err(|err| format!("reading the pane failed: {err}"))
                    .and_then(|rex| read(&rex, &host, &session, &block));
                let _ = tx.send(Msg::Screen(key, screen));
            });
        }
        Effect::Send {
            key,
            pane,
            expect,
            action,
            what,
        } => {
            thread::spawn(move || {
                let result = rex::locate()
                    .map_err(|err| format!("send failed: {err}; nothing sent"))
                    .and_then(|rex| send::send(&rex, &pane, &expect, &action));
                // A reply marks the row seen, as a jump does.
                if result.is_ok() {
                    let _ = client::call(&json!({ "cmd": "seen", "key": key }));
                }
                let _ = tx.send(Msg::Sent { key, what, result });
            });
        }
        Effect::SavePrefs(prefs) => {
            let saved = tui_json()
                .ok_or_else(|| "no state dir".to_owned())
                .and_then(|path| {
                    prefs::save(&path, &prefs).map_err(|err| format!("{}: {err}", path.display()))
                });
            if let Err(why) = saved {
                let _ = tx.send(Msg::Say(format!("could not keep the card setting: {why}")));
            }
        }
        Effect::Reconnect => listen(&tx, generation),
        Effect::Quit => {}
    }
}

pub(super) fn tui_json() -> Option<std::path::PathBuf> {
    Paths::find().ok().map(|paths| paths.tui())
}

/// Starts a subscriber thread. A newer `listen` makes the older one stop at its next tick.
pub(super) fn listen(tx: &Sender<Msg>, generation: &Arc<AtomicU64>) {
    let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, generation) = (tx.clone(), generation.clone());
    thread::spawn(move || {
        let current = || generation.load(Ordering::SeqCst) == mine;
        let result = client::subscribe(|rows| {
            if let Some(rows) = rows
                && (!current() || tx.send(Msg::Rows(rows)).is_err())
            {
                return false;
            }
            current()
        });
        if current() {
            let why = match result {
                Err(Error::Serve { message, .. }) if message == "serve stopped" => {
                    "serve stopped".to_owned()
                }
                Err(err) => format!("serve: {err}"),
                Ok(()) => "serve stopped".to_owned(),
            };
            let _ = tx.send(Msg::Gone(why));
        }
    });
}

/// What a peek shows: the pane's last lines, or why there are none. None when a newer peek
/// cancelled it, which then owns the slot.
fn peek(
    rex: &std::path::Path,
    host: &str,
    session: &str,
    block: &str,
    vt: bool,
    cancelled: &dyn Fn() -> bool,
) -> Option<std::result::Result<Vec<String>, String>> {
    let (bytes, format) = if vt {
        (PEEK_VT_BYTES, "vt")
    } else {
        (PEEK_BYTES, "text")
    };
    let args =
        json!({ "session_id": session, "block_id": block, "max_bytes": bytes, "format": format });
    match capture(rex, host, &args, rex::TAIL, cancelled) {
        Ok(reply) => Some(Ok(peek_lines(
            reply["text"].as_str().unwrap_or_default(),
            vt,
        ))),
        Err(Error::Cancelled) => None,
        Err(err) => Some(Err(said(err, "peek failed"))),
    }
}

/// `show --tail`: the last `lines` of a row's pane as the card reads them, through the board's
/// own bounded read.
pub(crate) fn tail(row: &Row, lines: usize) -> Result<Vec<String>> {
    let rex = rex::locate()?;
    let p = &row.pane;
    match peek(&rex, &p.host, &p.session_id, &p.block_id, false, &|| false) {
        Some(Ok(all)) => Ok(all[all.len().saturating_sub(lines)..].to_vec()),
        Some(Err(why)) => Err(Error::Rex(why)),
        None => Err(Error::Cancelled),
    }
}

/// `answer`: the row's screen, read as answer mode reads it.
pub(crate) fn screen(row: &Row) -> Result<Screen> {
    let rex = rex::locate()?;
    let p = &row.pane;
    read(&rex, &p.host, &p.session_id, &p.block_id).map_err(Error::Rex)
}

/// What answer mode reads: the tail as the card reads it, and the input line with its styles.
fn read(
    rex: &std::path::Path,
    host: &str,
    session: &str,
    block: &str,
) -> std::result::Result<Screen, String> {
    let args = json!({ "session_id": session, "block_id": block, "max_bytes": PEEK_BYTES });
    let reply = capture(rex, host, &args, rex::READ, &|| false)
        .map_err(|err| said(err, "reading the pane failed"))?;
    Ok(Screen::parse(
        reply["text"].as_str().unwrap_or_default(),
        reply["vt"].as_str(),
    ))
}

fn said(err: Error, what: &str) -> String {
    match err {
        Error::Io(err) if err.kind() == std::io::ErrorKind::TimedOut => {
            format!("the pane did not answer within {}s", PEEK_WITHIN.as_secs())
        }
        err => format!("{what}: {err}"),
    }
}

/// Runs a tail script on the pane's host: only the end of its scrollback is read (`tail.lua`),
/// within PEEK_WITHIN, and `cancelled` stops it early.
fn capture(
    rex: &std::path::Path,
    host: &str,
    args: &serde_json::Value,
    lua: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<serde_json::Value> {
    let remote = (host != "local").then_some(host);
    let reply = rex::run_script_with(rex, remote, lua, args, PEEK_WITHIN, cancelled)?;
    if reply["ok"] != true {
        let field = |name: &str| reply[name].as_str().unwrap_or("unknown").to_owned();
        return Err(Error::Rex(format!("{}: {}", field("step"), field("error"))));
    }
    Ok(reply)
}

/// The last lines of captured pane text, with every escape sequence and control character
/// removed but, for `vt`, the colours: the text comes from whatever runs in that pane. Trailing
/// blank lines go too, so a padded line does not wrap into empty rows.
fn peek_lines(text: &str, vt: bool) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let kept = if vt { vt::sanitize(line) } else { clean(line) };
            kept.trim_end().to_owned()
        })
        .collect();
    while lines
        .last()
        .is_some_and(|line| clean(line).trim().is_empty())
    {
        lines.pop();
    }
    lines[lines.len().saturating_sub(PEEK_LINES)..].to_vec()
}

/// What `s` reports. A blocked or erroring pane stays in NEEDS YOU, so it says why.
fn seen_message(label: &str, state: &str) -> String {
    match state {
        "blocked" => format!("{label} marked seen; it stays in NEEDS YOU while blocked"),
        "error" => format!("{label} marked seen; it stays in NEEDS YOU while in error"),
        _ => format!("{label} marked seen"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rex that logs its argv and pid to `dir`, sleeps `sleep` seconds, then prints `reply`.
    fn fake_rex(
        dir: &std::path::Path,
        reply: &serde_json::Value,
        sleep: u32,
    ) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("reply"), reply.to_string()).unwrap();
        let rex = dir.join("rex");
        let d = dir.display();
        std::fs::write(
            &rex,
            format!(
                "#!/bin/sh\necho \"$@\" > '{d}/argv'\necho $$ > '{d}/pid'\ncat > /dev/null\nsleep {sleep}\ncat '{d}/reply'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&rex, std::fs::Permissions::from_mode(0o755)).unwrap();
        rex
    }

    fn gone(dir: &std::path::Path) -> bool {
        let pid: i32 = std::fs::read_to_string(dir.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        unsafe { libc::kill(pid, 0) != 0 }
    }

    #[test]
    fn a_peek_reads_the_tail_of_the_pane_on_its_own_host() {
        let dir = tempfile::tempdir().unwrap();
        let mut text: String = (1..=250).map(|n| format!("line {n}\n")).collect();
        text.push_str("\x1b[31mRED\x1b[0m\n\n  \n");
        let rex = fake_rex(
            dir.path(),
            &json!({ "ok": true, "text": text, "total": 9000, "cut": true }),
            0,
        );
        let lines = peek(&rex, "mini", "session:1", "block:2", false, &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(lines.len(), PEEK_LINES);
        assert_eq!(lines.last().unwrap(), "RED", "trailing blank lines dropped");
        assert!(
            lines.iter().all(|l| !l.chars().any(char::is_control)),
            "{lines:?}"
        );
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        let (head, args) = argv.trim().split_once(" --args ").unwrap();
        assert_eq!(head, "-S mini do - --json");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(args).unwrap(),
            json!({ "session_id": "session:1", "block_id": "block:2", "max_bytes": 49152, "format": "text" })
        );
        peek(&rex, "local", "session:1", "block:2", false, &|| false);
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        assert!(
            argv.starts_with("do - --json --args "),
            "no -S for this host: {argv}"
        );
    }

    #[test]
    fn a_full_view_peek_reads_vt_and_keeps_only_the_colours() {
        let dir = tempfile::tempdir().unwrap();
        let text = "\x1b[1C\x1b[0m\x1b[38;2;255;0;0m-\x1b[1Cold\x1b[0m\x1b]0;t\x07\r\n\x1b[2K\r\n";
        let rex = fake_rex(dir.path(), &json!({ "ok": true, "text": text }), 0);
        let lines = peek(&rex, "local", "s", "b", true, &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(lines, [" \x1b[0m\x1b[38;2;255;0;0m- old\x1b[0m"]);
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        let args: serde_json::Value =
            serde_json::from_str(argv.trim().split_once(" --args ").unwrap().1).unwrap();
        assert_eq!(
            (&args["format"], &args["max_bytes"]),
            (&json!("vt"), &json!(98304))
        );
    }

    #[test]
    fn a_failed_tail_says_which_step() {
        let dir = tempfile::tempdir().unwrap();
        let reply = json!({ "ok": false, "step": "blob_read", "error": "not_found" });
        let rex = fake_rex(dir.path(), &reply, 0);
        assert_eq!(
            peek(&rex, "local", "s", "b", false, &|| false),
            Some(Err("peek failed: blob_read: not_found".to_owned()))
        );
    }

    #[test]
    fn a_slow_host_gives_up_after_3s_and_its_rex_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true, "text": "late" }), 5);
        let started = std::time::Instant::now();
        let got = peek(&rex, "mini", "s", "b", false, &|| false);
        let took = started.elapsed();
        assert_eq!(
            got,
            Some(Err("the pane did not answer within 3s".to_owned()))
        );
        assert!(
            took >= PEEK_WITHIN && took < Duration::from_secs(4),
            "{took:?}"
        );
        assert!(gone(dir.path()), "rex was killed");
    }

    #[test]
    fn a_newer_peek_cancels_the_older_one_and_its_rex_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true, "text": "stale" }), 5);
        let started = std::time::Instant::now();
        let newer = std::sync::atomic::AtomicBool::new(false);
        let got = thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(200));
                newer.store(true, Ordering::SeqCst);
            });
            peek(&rex, "local", "s", "b", false, &|| {
                newer.load(Ordering::SeqCst)
            })
        });
        assert_eq!(got, None, "nothing to apply");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "stopped well before the limit"
        );
        assert!(gone(dir.path()), "rex was killed");
    }

    #[test]
    fn a_line_padded_with_blanks_takes_one_row() {
        let lines = peek_lines(&format!("foo{}\n", " ".repeat(120)), false);
        assert_eq!(lines, ["foo"]);
        assert_eq!(
            crate::output::chop(&lines[0], 45).len(),
            1,
            "one row at 45 columns"
        );
    }

    #[test]
    fn peek_lines_keep_the_last_200_without_escape_sequences() {
        let mut text: String = (1..=250).map(|n| format!("line {n}\n")).collect();
        text.push_str("\x1b[31mRED\x1b[0m\n");
        text.push_str("clip \x1b]52;c;aGVsbG8=\x07end\n");
        text.push_str("title \x1b]0;set by the pane\x1b\\end\n");
        text.push_str("c1 \u{9b}2Jend\n");
        text.push_str("tab\there\r\n");
        let lines = peek_lines(&text, false);
        assert_eq!(lines.len(), PEEK_LINES);
        assert_eq!(lines.last().unwrap(), "tab here");
        assert!(
            lines.iter().all(|l| !l.chars().any(char::is_control)),
            "{lines:?}"
        );
        assert!(lines.contains(&"RED".to_owned()), "{lines:?}");
        assert!(lines.contains(&"clip end".to_owned()), "{lines:?}");
        assert!(lines.contains(&"title end".to_owned()), "{lines:?}");
        assert!(lines.contains(&"c1 end".to_owned()), "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| !l.contains("[31m") && !l.contains("]52;")),
            "{lines:?}"
        );
    }

    #[test]
    fn seen_says_when_a_row_stays_in_needs_you() {
        assert_eq!(seen_message("seat-a", "done"), "seat-a marked seen");
        assert_eq!(
            seen_message("seat-a", "blocked"),
            "seat-a marked seen; it stays in NEEDS YOU while blocked"
        );
    }
}
