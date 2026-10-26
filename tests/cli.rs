use assert_cmd::Command;
use predicates::prelude::*;

/// A path no test can create, so a command that would reach serve without a `Serve` fails
/// instead of touching the real one.
const NO_SERVE: &str = "/nonexistent/rexboard-tests";

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("rexboard").expect("binary");
    cmd.env("XDG_RUNTIME_DIR", NO_SERVE)
        .env("XDG_STATE_HOME", NO_SERVE)
        .env("XDG_CONFIG_HOME", NO_SERVE)
        .env("TMPDIR", NO_SERVE);
    cmd
}

#[test]
fn version_prints_name() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("rexboard {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_flag_with_json_is_usage_json() {
    bin()
        .args(["--nope", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"code\":\"USAGE\""))
        .stderr(predicate::str::contains("--nope"));
}

#[test]
fn no_command_prints_help_and_exits_2() {
    bin()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn no_command_with_json_is_usage_json() {
    bin()
        .arg("--json")
        .assert()
        .code(2)
        .stderr(
            "{\"error\":{\"code\":\"USAGE\",\"message\":\"a command is required\",\"next_steps\":[\"rexboard --help\"]},\"ok\":false}\n",
        );
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn ls() -> Command {
    let mut cmd = bin();
    cmd.env("REXBOARD_REX", fixture("fake-rex"))
        .args(["ls", "--direct"]);
    cmd
}

fn ls_json(cmd: &mut Command) -> Vec<serde_json::Value> {
    let out = cmd
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).expect("ls --json prints a JSON array")
}

fn by_block<'a>(panes: &'a [serde_json::Value], block: &str) -> &'a serde_json::Value {
    panes
        .iter()
        .find(|pane| pane["block_id"] == block)
        .unwrap_or_else(|| panic!("no pane {block}"))
}

fn fixture_with_mini(mini: serde_json::Value) -> tempfile::NamedTempFile {
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("snapshot-two-hosts.json")).unwrap())
            .unwrap();
    snapshot["hosts"][1] = mini;
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), snapshot.to_string()).unwrap();
    file
}

#[test]
fn ls_json_lists_panes_from_both_hosts() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let panes = ls_json(ls().env("FAKE_REX_LOG", log.path()));
    let blocks: Vec<_> = panes
        .iter()
        .map(|p| p["block_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        blocks,
        [
            "block:web-seat-b",
            "block:api-claude",
            "block:web-codex",
            "block:web-rig-review",
            "block:api-seat-a",
            "block:web-codex-seat"
        ]
    );
    let codex_seat = by_block(&panes, "block:web-codex-seat");
    assert_eq!(
        codex_seat["harness"], "codex",
        "a Codex title with no record"
    );
    assert_eq!(codex_seat["source"], "inferred");
    assert_eq!(codex_seat["state"], "unknown");
    assert_eq!(codex_seat["group"], "unreported");
    let claude = by_block(&panes, "block:api-claude");
    assert_eq!(claude["host"], "local");
    assert_eq!(claude["session"], "api");
    assert_eq!(claude["tab"], "main");
    assert_eq!(claude["harness"], "claude-code");
    assert_eq!(claude["state"], "done");
    assert_eq!(claude["source"], "native");
    assert_eq!(claude["msg"], "Tests pass");
    assert_eq!(claude["children"], serde_json::json!({ "working": 1 }));
    assert_eq!(by_block(&panes, "block:web-codex")["host"], "mini");
    assert_eq!(by_block(&panes, "block:web-codex")["harness"], "codex");
    let argv = std::fs::read_to_string(log.path()).unwrap();
    assert!(argv.starts_with("do - --json --args "), "{argv}");
}

#[test]
fn ls_marks_title_only_seat_inferred() {
    let panes = ls_json(&mut ls());
    let seat = by_block(&panes, "block:api-seat-a");
    assert_eq!(seat["source"], "inferred");
    assert_eq!(seat["state"], "idle");
    assert_eq!(seat["harness"], "claude-code");
    assert_eq!(seat["updated_at"], serde_json::Value::Null);
    assert!(panes.iter().all(|p| p["block_id"] != "block:api-shell"));
    let all = ls_json(ls().arg("--all"));
    assert_eq!(by_block(&all, "block:api-shell")["state"], "unknown");
}

#[test]
fn ls_marks_tmux_owner_as_bridge() {
    let panes = ls_json(&mut ls());
    let seat = by_block(&panes, "block:web-seat-b");
    assert_eq!(seat["source"], "bridge");
    assert_eq!(seat["state"], "blocked");
    assert_eq!(seat["kind"], "permission");
}

#[test]
fn ls_unreachable_host_warns_and_lists_the_rest() {
    let snapshot =
        fixture_with_mini(serde_json::json!({ "host": "mini", "error": "dial timeout" }));
    let out = ls()
        .env("FAKE_REX_FIXTURE", snapshot.path())
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: mini is unreachable: dial timeout",
        ))
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("GROUP"), "{text}");
    let done = text.lines().find(|l| l.contains("Tests pass")).unwrap();
    assert!(
        done.contains("api › main ") && !done.contains("shell"),
        "a generic pane label gives way to the tab: {done}"
    );
    assert!(!text.contains("mini"), "{text}");
}

#[test]
fn ls_every_host_down_is_hosts_unreachable() {
    let snapshot = serde_json::json!({ "hosts": [
        { "host": "local", "error": "no server" },
        { "host": "mini", "error": "dial timeout" }
    ]});
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), snapshot.to_string()).unwrap();
    ls().env("FAKE_REX_FIXTURE", file.path())
        .arg("--json")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"code\":\"HOSTS_UNREACHABLE\""));
}

#[test]
fn ls_record_without_state_is_a_rex_error() {
    let snapshot = fixture_with_mini(serde_json::json!({ "host": "mini", "sessions": [{
        "session_id": "session:web", "label": "web",
        "windows": [{ "window_id": "window:web", "label": "seats", "blocks": [{
            "block_id": "block:web", "label": "seat-b", "title": "", "process": {},
            "records": [{ "app": "claude-code" }]
        }]}]
    }]}));
    ls().env("FAKE_REX_FIXTURE", snapshot.path())
        .arg("--json")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"REX\""))
        .stderr(predicate::str::contains("missing field `state`"));
}

#[test]
fn ls_text_drops_the_title_glyph() {
    ls().assert()
        .success()
        .stdout(predicate::str::is_match(r"api › main › seat-a +seat-a@demo\n").unwrap())
        .stdout(predicate::str::contains("✳").not());
}

#[test]
fn ls_without_rex_errors_with_next_steps() {
    let home = tempfile::tempdir().unwrap();
    bin()
        .args(["ls", "--json", "--direct"])
        .env_remove("REXBOARD_REX")
        .env_remove("REX_BIN_DIR")
        .env("PATH", home.path())
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"REX_NOT_FOUND\""))
        .stderr(predicate::str::contains("set REXBOARD_REX"));
}

/// Runs `rexboard watch ARGS` against fake-rex until `done` holds for the lines so far, then
/// stops it with SIGTERM. Fails after 30s, so the test never depends on a short time window.
fn watch_until(
    args: &[&str],
    envs: &[(&str, &str)],
    done: impl Fn(&[serde_json::Value]) -> bool,
) -> (Vec<serde_json::Value>, String) {
    use std::io::{BufRead, BufReader, Read};
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rexboard"))
        .arg("watch")
        .args(args)
        .env("REXBOARD_REX", fixture("fake-rex"))
        .env("XDG_RUNTIME_DIR", NO_SERVE)
        .env("XDG_STATE_HOME", NO_SERVE)
        .env("TMPDIR", NO_SERVE)
        .envs(envs.iter().copied())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).unwrap();
        text
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut lines = Vec::new();
    while !done(&lines) {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => lines.push(serde_json::from_str(&line).expect("watch prints JSON lines")),
            Err(_) => {
                let _ = child.kill();
                panic!("watch did not reach the end marker in 30s; got {lines:#?}");
            }
        }
    }
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert!(child.wait().unwrap().success(), "watch exits 0 on SIGTERM");
    (lines, errors.join().unwrap())
}

fn count(lines: &[serde_json::Value], host: &str, ty: &str) -> usize {
    lines
        .iter()
        .filter(|line| line["host"] == host && line["type"] == ty)
        .count()
}

#[test]
fn watch_raw_merges_two_hosts() {
    let (lines, _) = watch_until(&["--raw"], &[], |lines| {
        count(lines, "local", "tick") == 1 && count(lines, "mini", "tick") == 1
    });
    let of = |host: &str| -> Vec<&str> {
        lines
            .iter()
            .filter(|line| line["host"] == host)
            .map(|line| line["type"].as_str().unwrap())
            .collect()
    };
    let recorded = |name: &str| -> Vec<String> {
        std::fs::read_to_string(fixture(name))
            .unwrap()
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["type"].to_string()
            })
            .map(|ty| ty.trim_matches('"').to_owned())
            .collect()
    };
    assert_eq!(of("local"), recorded("observer-studio.ndjson"));
    assert_eq!(of("mini"), recorded("observer-mini.ndjson"));
    let blocked = lines
        .iter()
        .find(|line| line["host"] == "mini" && line["record"]["state"] == "blocked")
        .unwrap();
    assert_eq!(blocked["record"]["kind"], "permission");
    assert_eq!(blocked["session_id"], "session:web-probe");
}

#[test]
fn watch_raw_restarts_observer_after_exit() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let log_path = log.path().to_str().unwrap();
    let (lines, _) = watch_until(
        &["--raw", "--host", "local"],
        &[("FAKE_REX_EXIT_AFTER", "2"), ("FAKE_REX_LOG", log_path)],
        |lines| count(lines, "local", "down") == 2,
    );
    let types: Vec<_> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        [
            "snapshot",
            "session_created",
            "down",
            "snapshot",
            "session_created",
            "down"
        ]
    );
    assert!(
        lines
            .iter()
            .all(|line| line["host"] == "local" && line["v"] == 1)
    );
    assert_eq!(lines[2]["retry_in_s"], 1);
    assert_eq!(lines[2]["reason"], "observer exited (exit status: 0)");
    assert_eq!(
        lines[5]["retry_in_s"], 2,
        "a short run must not reset the backoff"
    );
    let argv = std::fs::read_to_string(log.path()).unwrap();
    assert_eq!(argv.matches("do - --all --fail-fast").count(), 2, "{argv}");
}

#[test]
fn watch_raw_down_reason_is_observer_stderr() {
    let (lines, _) = watch_until(
        &["--raw", "--host", "local"],
        &[
            ("FAKE_REX_EXIT_AFTER", "1"),
            (
                "FAKE_REX_STDERR",
                "observer.lua:12: attempt to index a nil value",
            ),
        ],
        |lines| count(lines, "local", "down") == 1,
    );
    assert_eq!(
        lines.last().unwrap()["reason"],
        "observer exited (exit status: 1): observer.lua:12: attempt to index a nil value"
    );
}

#[test]
fn watch_raw_passes_unknown_types_through() {
    let (lines, _) = watch_until(
        &["--raw", "--host", "local"],
        &[(
            "FAKE_REX_APPEND",
            r#"{"v":1,"type":"bell","session_id":"session:api"}"#,
        )],
        |lines| count(lines, "local", "bell") == 1,
    );
    assert_eq!(
        *lines.last().unwrap(),
        serde_json::json!({ "v": 1, "type": "bell", "session_id": "session:api", "host": "local" })
    );
}

#[test]
fn watch_raw_restarts_a_host_after_an_unreadable_line() {
    // Shape Rex sent for a detached block (window_id null) before the observer filtered them.
    let bad = r#"{"v":1,"type":"block_created","session_id":"session:api","block":{"block_id":"block:api-detached"}}"#;
    let (lines, errors) = watch_until(
        &["--raw", "--host", "local"],
        &[("FAKE_REX_APPEND", bad)],
        |lines| count(lines, "local", "snapshot") == 2,
    );
    let down = lines.iter().find(|line| line["type"] == "down").unwrap();
    assert!(
        down["reason"]
            .as_str()
            .unwrap()
            .contains("missing field `window_id`"),
        "{down}"
    );
    assert!(
        errors.contains("warning: local: unexpected observer line from Rex 0.0.1792787366-g623b2d: missing field `window_id`; restarting its observer: {\"v\":1,\"type\":\"block_created\""),
        "{errors}"
    );
    assert!(!errors.contains("will not restart"), "{errors}");
}

#[test]
fn watch_without_raw_is_usage() {
    bin()
        .env("REXBOARD_REX", fixture("fake-rex"))
        .arg("watch")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("watch needs --json or --raw"));
}

#[test]
fn ls_json_adds_board_fields() {
    let panes = ls_json(&mut ls());
    let claude = by_block(&panes, "block:api-claude");
    assert_eq!(claude["since"], "2026-01-01T10:00:00.000Z");
    assert_eq!(claude["since_estimated"], false);
    assert_eq!(claude["seen"], false);
    assert_eq!(claude["stale"], false);
    assert_eq!(claude["group"], "needs_you");
    assert_eq!(claude["last_turn"], serde_json::Value::Null);
    let codex = by_block(&panes, "block:web-codex");
    assert_eq!(
        codex["since_estimated"], true,
        "a seeded working row is a guess"
    );
    assert_eq!(codex["turn_started"], "2026-01-01T10:02:00.000Z");
    assert_eq!(by_block(&panes, "block:api-seat-a")["group"], "unreported");
    assert_eq!(codex["process"], "codex");
    assert!(panes.iter().all(|p| p.as_object().unwrap().len() == 26));
}

/// A status line for the fixture's idle seat on `host`, sent after the recorded stream ends.
fn seat_done(session: &str, block: &str) -> String {
    serde_json::json!({ "v": 1, "type": "status", "session_id": session, "block_id": block,
        "record_id": "", "record": { "state": "done", "app": "claude-code",
        "updated_at": "2026-01-01T00:00:00Z" } })
    .to_string()
}

fn rows_where(prints: &[serde_json::Value], block: &str, state: &str) -> bool {
    prints.last().is_some_and(|rows| {
        rows.as_array()
            .unwrap()
            .iter()
            .any(|row| row["block_id"] == block && row["state"] == state)
    })
}

#[test]
fn watch_json_prints_debounced_rows() {
    let marker = seat_done("session:web", "block:web-seat-b");
    let (prints, _) = watch_until(
        &["--json", "--direct", "--host", "mini"],
        &[("FAKE_REX_APPEND_mini", &marker)],
        |prints| rows_where(prints, "block:web-seat-b", "done"),
    );
    assert!(!prints.is_empty());
    let last = prints.last().unwrap().as_array().unwrap();
    assert_eq!(
        last.len(),
        1,
        "the probe pane closed, only the seat is left"
    );
    assert_eq!(last[0]["host"], "mini");
    assert_eq!(last[0]["group"], "needs_you");
    assert_eq!(last[0]["source"], "bridge");
}

#[test]
fn watch_json_restarts_a_host_that_sends_a_bad_record() {
    let bad = r#"{"v":1,"type":"status","session_id":"session:web","block_id":"block:web-seat-b","record_id":"","record":{"app":"x"}}"#;
    let marker = seat_done("session:api", "block:api-seat-a");
    let log = tempfile::NamedTempFile::new().unwrap();
    let mini_starts = || {
        std::fs::read_to_string(log.path())
            .unwrap()
            .matches("-S mini do - --all")
            .count()
    };
    let (_, errors) = watch_until(
        &["--json", "--direct"],
        &[
            ("FAKE_REX_APPEND_mini", bad),
            ("FAKE_REX_APPEND_studio", &marker),
            ("FAKE_REX_LOG", log.path().to_str().unwrap()),
        ],
        |prints| mini_starts() >= 2 && rows_where(prints, "block:api-seat-a", "done"),
    );
    assert!(
        errors.contains(
            "warning: mini: unexpected pane shape from Rex 0.0.1792787366-g623b2d: missing field `state`; restarting its observer: {"
        ),
        "{errors}"
    );
    assert!(!errors.contains("will not restart"), "{errors}");
}

#[test]
fn ls_warns_on_unreadable_updated_at() {
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("snapshot-two-hosts.json")).unwrap())
            .unwrap();
    snapshot["hosts"][1]["sessions"][0]["windows"][0]["blocks"][0]["records"][0]["updated_at"] =
        "yesterday".into();
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), snapshot.to_string()).unwrap();
    let out = ls()
        .env("FAKE_REX_FIXTURE", file.path())
        .arg("--json")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            r#"warning: mini: updated_at "yesterday" is not RFC 3339 (Rex version unknown); time in state is estimated"#,
        ))
        .get_output()
        .stdout
        .clone();
    let panes: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        by_block(&panes, "block:web-seat-b")["since_estimated"],
        true
    );
}

#[test]
fn ls_text_shows_group_and_estimated_age() {
    ls().assert()
        .success()
        .stdout(predicate::str::starts_with("GROUP"))
        .stdout(predicate::str::is_match(r"\nworking +working +≥\d+d\d+h +codex").unwrap());
}

/// A serve with its own runtime and state dirs, stopped when the test ends.
struct Serve {
    dir: tempfile::TempDir,
    log: std::path::PathBuf,
}

impl Serve {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("rb").tempdir().unwrap();
        std::fs::create_dir(dir.path().join("run")).unwrap();
        let log = dir.path().join("fake-rex.log");
        let serve = Self { dir, log };
        assert!(
            serve.socket().as_os_str().len() <= 103,
            "temp socket path too long"
        );
        serve
    }

    fn envs(&self) -> Vec<(&'static str, String)> {
        let path = |p: std::path::PathBuf| p.to_str().unwrap().to_owned();
        vec![
            ("XDG_STATE_HOME", path(self.dir.path().join("state"))),
            ("XDG_CONFIG_HOME", path(self.dir.path().join("config"))),
            ("TMPDIR", path(self.dir.path().to_owned())),
            ("REXBOARD_REX", path(fixture("fake-rex"))),
            ("FAKE_REX_LOG", path(self.log.clone())),
        ]
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("rexboard").unwrap();
        cmd.env_remove("XDG_RUNTIME_DIR").envs(self.envs());
        cmd
    }

    fn socket(&self) -> std::path::PathBuf {
        self.file("serve.sock")
    }

    fn config(&self, toml: &str) {
        let dir = self.dir.path().join("config/rexboard");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), toml).unwrap();
    }

    fn log_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The args of each jump script run, from the fake-rex log.
    fn jumps(&self) -> Vec<serde_json::Value> {
        self.log_lines()
            .iter()
            .filter_map(|line| line.strip_prefix("do - --json --args "))
            .filter_map(|args| serde_json::from_str::<serde_json::Value>(args).ok())
            .filter(|args| args.get("block_id").is_some())
            .collect()
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join("state/rexboard").join(name)
    }

    fn pid(&self) -> i64 {
        let text = std::fs::read_to_string(self.file("serve.lock")).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["pid"]
            .as_i64()
            .unwrap()
    }

    fn observer_starts(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .matches("do - --all --fail-fast")
            .count()
    }

    fn rows(&self, extra: &[&str]) -> Vec<serde_json::Value> {
        let out = self
            .cmd()
            .args(["ls", "--json"])
            .args(extra)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).unwrap()
    }
}

impl Drop for Serve {
    /// Stops until serve.lock stays free for 200ms: a spare serve that a racing command started
    /// can take the lock after the first stop.
    fn drop(&mut self) {
        let lock = self.file("serve.lock");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let _ = self.cmd().args(["serve", "--stop"]).ok();
            let free_since = std::time::Instant::now();
            while !held(&lock) {
                if free_since.elapsed() >= std::time::Duration::from_millis(200) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
}

/// Whether something holds the flock on `path`, as rexboard's own probe asks.
fn held(path: &std::path::Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    rc != 0
}

/// Polls `check` every 20ms for up to 10s.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !check() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn row<'a>(rows: &'a [serde_json::Value], key: &str) -> &'a serde_json::Value {
    rows.iter()
        .find(|row| row["key"] == key)
        .unwrap_or_else(|| panic!("no row {key} in {rows:#?}"))
}

/// A mini observer stream whose snapshot holds an unseen done pane and a quiet seat.
fn mini_with_done(serve: &Serve) -> String {
    let snapshot = serde_json::json!({ "v": 1, "type": "snapshot", "rex_version": "0.0.1-test",
        "sessions": [{ "session_id": "session:web", "label": "web", "windows": [{
            "window_id": "window:web", "label": "main", "blocks": [
                { "block_id": "block:web-done", "label": "shell", "title": "", "process": {},
                  "records": [{ "state": "done", "app": "claude-code", "msg": "Tests pass",
                                "updated_at": "2026-01-01T00:00:00Z" }] },
                { "block_id": "block:web-quiet", "label": "seat-q", "title": "✳ seat-q@demo",
                  "process": { "name": "tmux" }, "records": {} }
            ] }] }] });
    let path = serve.dir.path().join("observer-mini.ndjson");
    std::fs::write(&path, format!("{snapshot}\n")).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn serve_is_found_from_any_tmpdir() {
    let serve = Serve::new();
    let ls = |tmp: &str| {
        let dir = serve.dir.path().join(tmp);
        std::fs::create_dir_all(&dir).unwrap();
        let out = serve
            .cmd()
            .env("TMPDIR", &dir)
            .args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<Vec<serde_json::Value>>(&out).unwrap()
    };
    assert!(!ls("tmp-a").is_empty());
    let pid = serve.pid();
    assert!(
        !ls("tmp-b").is_empty(),
        "a caller with another TMPDIR gets rows"
    );
    assert_eq!(serve.pid(), pid, "and reaches the same serve");
    assert!(serve.socket().exists(), "the socket sits beside serve.lock");
    assert_eq!(
        serve.observer_starts(),
        2,
        "no second serve started observers"
    );
}

#[test]
fn a_dead_serve_leaves_nothing_that_stops_the_next() {
    let serve = Serve::new();
    std::fs::create_dir_all(serve.file("")).unwrap();
    let record = serde_json::json!({ "pid": 999_999, "rex": fixture("fake-rex"), "children": [] });
    std::fs::write(serve.file("serve.lock"), record.to_string()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(serve.socket()).unwrap());
    assert!(serve.socket().exists(), "a socket nobody listens on");
    assert!(!serve.rows(&[]).is_empty());
    assert_ne!(serve.pid(), 999_999, "a new serve took the lock");
}

/// A process that holds serve.lock, as a serve would, with no socket: an older build's serve
/// listening elsewhere, or a stuck one. It runs under `sh`, which reaps it, so like a real serve
/// it is not the test's child. Returns `sh` and the holder's pid.
fn hold_lock(serve: &Serve) -> (std::process::Child, i32) {
    std::fs::create_dir_all(serve.file("")).unwrap();
    let lock = serve.file("serve.lock");
    let perl = r#"use Fcntl ':flock'; open F, '>>', $ARGV[0] or die; flock F, LOCK_EX or die; $| = 1; print "$$\n"; sleep 30"#;
    let mut sh = std::process::Command::new("sh")
        .args(["-c", r#"perl -e "$1" "$0" & wait"#])
        .arg(&lock)
        .arg(perl)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(sh.stdout.take().unwrap()),
        &mut line,
    )
    .unwrap();
    let pid: i32 = line.trim().parse().unwrap();
    let record = serde_json::json!({ "pid": pid, "rex": fixture("fake-rex"), "children": [] });
    std::fs::write(&lock, record.to_string()).unwrap();
    (sh, pid)
}

#[test]
fn a_lock_held_by_another_program_is_left_alone() {
    let serve = Serve::new();
    let (mut sh, pid) = hold_lock(&serve);
    let out = serve
        .cmd()
        .args(["ls", "--json"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let text = String::from_utf8_lossy(&out);
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    sh.wait().unwrap();
    assert!(alive, "the holder was not signalled");
    assert!(
        text.contains(&format!(
            "serve.lock names pid {pid}, which does not answer and is not rexboard, so it was left alone"
        )),
        "{text}"
    );
    assert!(text.contains("kill <the pid lsof shows>"), "{text}");
    assert!(!text.contains("restarted serve"), "{text}");
    assert_eq!(serve.observer_starts(), 0, "no second serve was started");
}

/// The lines `rexboard ls --json` printed on stderr, and its rows.
fn ls_with_stderr(cmd: &mut Command) -> (String, Vec<serde_json::Value>) {
    let out = cmd
        .args(["ls", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    (
        String::from_utf8_lossy(&out.stderr).into_owned(),
        serde_json::from_slice(&out.stdout).unwrap(),
    )
}

#[test]
fn a_rexboard_serve_that_does_not_answer_is_replaced_once() {
    let serve = Serve::new();
    serve.rows(&[]);
    let old = serve.pid();
    // Still serve, still holding the lock, but nobody can reach it, as after an upgrade that
    // moved the socket.
    std::fs::remove_file(serve.socket()).unwrap();
    let (said, rows) = ls_with_stderr(&mut serve.cmd());
    assert!(!rows.is_empty());
    assert!(
        said.contains(&format!("restarted serve (pid {old} did not answer) -> ")),
        "{said}"
    );
    assert_ne!(
        unsafe { libc::kill(old as i32, 0) },
        0,
        "serve {old} is gone"
    );
    assert_ne!(serve.pid(), old, "a new serve holds the lock");
    let log = std::fs::read_to_string(serve.file("serve.log")).unwrap();
    assert_eq!(log.matches("restarted serve").count(), 1, "{log}");
    let (said, _) = ls_with_stderr(&mut serve.cmd());
    assert!(!said.contains("restarted"), "only once: {said}");
}

/// Runs `ls --json` 12 times at once; the stderr of each one that failed.
fn twelve_at_once(serve: &Serve) -> Vec<String> {
    let clients: Vec<_> = (0..12)
        .map(|_| {
            std::process::Command::new(assert_cmd::cargo::cargo_bin("rexboard"))
                .env_remove("XDG_RUNTIME_DIR")
                .envs(serve.envs())
                .args(["ls", "--json"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    clients
        .into_iter()
        .map(|client| client.wait_with_output().unwrap())
        .filter(|out| !out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stderr).into_owned())
        .collect()
}

fn restarts(serve: &Serve) -> usize {
    std::fs::read_to_string(serve.file("serve.log"))
        .unwrap_or_default()
        .matches("restarted serve")
        .count()
}

#[test]
fn many_commands_at_once_heal_a_silent_serve_once_and_all_get_rows() {
    let serve = Serve::new();
    serve.rows(&[]);
    std::fs::remove_file(serve.socket()).unwrap();
    let failed = twelve_at_once(&serve);
    assert!(failed.is_empty(), "every command got rows: {failed:#?}");
    assert_eq!(restarts(&serve), 1, "one restart");
}

#[test]
fn many_commands_at_once_replace_an_older_serve_once() {
    let serve = Serve::new();
    serve
        .cmd()
        .env("REXBOARD_TEST_VERSION", "0.0.1")
        .arg("serve")
        .assert()
        .success();
    let failed = twelve_at_once(&serve);
    assert!(failed.is_empty(), "every command got rows: {failed:#?}");
    assert_eq!(restarts(&serve), 1, "one restart");
}

#[test]
fn an_older_serve_is_restarted_once_and_a_newer_one_is_kept() {
    let serve = Serve::new();
    serve
        .cmd()
        .env("REXBOARD_TEST_VERSION", "0.0.1")
        .arg("serve")
        .assert()
        .success();
    let old = serve.pid();
    let (said, rows) = ls_with_stderr(&mut serve.cmd());
    assert!(!rows.is_empty());
    let mine = env!("CARGO_PKG_VERSION");
    assert!(
        said.contains(&format!("restarted serve 0.0.1 -> {mine}")),
        "{said}"
    );
    assert_ne!(serve.pid(), old, "a new serve holds the lock");
    let (said, _) = ls_with_stderr(&mut serve.cmd());
    assert!(
        !said.contains("restarted"),
        "the same version stays: {said}"
    );

    let newer = Serve::new();
    newer
        .cmd()
        .env("REXBOARD_TEST_VERSION", "99.0.0")
        .arg("serve")
        .assert()
        .success();
    let kept = newer.pid();
    let (said, _) = ls_with_stderr(&mut newer.cmd());
    assert!(!said.contains("restarted"), "a newer serve is kept: {said}");
    assert_eq!(newer.pid(), kept);
}

#[cfg(target_os = "macos")]
#[test]
fn serve_stop_finds_a_serve_whose_binary_was_deleted() {
    let serve = Serve::new();
    let bin_dir = tempfile::tempdir().unwrap();
    let copy = bin_dir.path().join("rexboard");
    std::fs::copy(assert_cmd::cargo::cargo_bin("rexboard"), &copy).unwrap();
    // Under sh, which reaps it, so like a real serve it is not the test's child.
    let mut sh = std::process::Command::new("sh")
        .args(["-c", r#""$0" serve --foreground >/dev/null 2>&1 & wait"#])
        .arg(&copy)
        .env_remove("XDG_RUNTIME_DIR")
        .envs(serve.envs())
        .spawn()
        .unwrap();
    eventually("the copied serve answers", || serve.socket().exists());
    let pid = serve.pid();
    // A failed assertion must not leave the copied serve running and cargo test waiting on sh.
    struct Kill(i64);
    impl Drop for Kill {
        fn drop(&mut self) {
            unsafe { libc::kill(self.0 as i32, libc::SIGKILL) };
        }
    }
    let _kill = Kill(pid);
    std::fs::remove_file(&copy).unwrap();
    std::fs::remove_file(serve.socket()).unwrap();
    serve
        .cmd()
        .args(["serve", "--stop"])
        .assert()
        .success()
        .stdout("serve stopped\n");
    sh.wait().unwrap();
    assert_ne!(
        unsafe { libc::kill(pid as i32, 0) },
        0,
        "serve {pid} is gone"
    );
}

#[test]
fn serve_stop_ends_a_serve_that_holds_the_lock_but_does_not_answer() {
    let serve = Serve::new();
    serve.rows(&[]);
    let pid = serve.pid();
    // Still serve, still holding the lock, but nobody can reach it.
    std::fs::remove_file(serve.socket()).unwrap();
    serve
        .cmd()
        .args(["serve", "--stop"])
        .assert()
        .success()
        .stdout("serve stopped\n");
    assert_ne!(
        unsafe { libc::kill(pid as i32, 0) },
        0,
        "serve {pid} is gone"
    );
}

#[test]
fn serve_stop_signals_nothing_when_the_lock_names_another_program() {
    let serve = Serve::new();
    let (mut sh, pid) = hold_lock(&serve);
    let out = serve
        .cmd()
        .args(["serve", "--stop"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let text = String::from_utf8_lossy(&out);
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    sh.wait().unwrap();
    assert!(alive, "the holder was not signalled");
    assert!(
        text.contains(&format!(
            "serve.lock names pid {pid}, which does not answer and is not rexboard, so it was left alone"
        )) && !text.contains("retry"),
        "the same refusal as any command's: {text}"
    );
}

#[test]
fn xdg_runtime_dir_does_not_split_callers() {
    let serve = Serve::new();
    let run = serve.dir.path().join("run");
    let ls = |cmd: &mut Command| {
        let out = cmd
            .args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<Vec<serde_json::Value>>(&out).unwrap()
    };
    assert!(!ls(serve.cmd().env("XDG_RUNTIME_DIR", &run)).is_empty());
    let pid = serve.pid();
    assert!(
        !ls(&mut serve.cmd()).is_empty(),
        "a caller without XDG_RUNTIME_DIR gets rows"
    );
    assert_eq!(serve.pid(), pid, "from the same serve");
    assert!(
        !run.join("rexboard.sock").exists(),
        "XDG_RUNTIME_DIR is not used"
    );
}

#[test]
fn serve_autostarts_and_answers_rows() {
    let serve = Serve::new();
    assert!(!serve.socket().exists());
    let rows = serve.rows(&[]);
    assert_eq!(row(&rows, "local/block:api-seat-a")["host"], "local");
    assert_eq!(row(&rows, "mini/block:web-seat-b")["host"], "mini");
    let pid = serve.pid();
    assert_eq!(serve.observer_starts(), 2, "one observer per host");
    serve.rows(&["--host", "mini"]);
    assert_eq!(serve.pid(), pid, "the second ls reuses serve");
    assert_eq!(serve.observer_starts(), 2, "and starts no new observers");
}

#[test]
fn serve_persists_seen_across_restart() {
    let serve = Serve::new();
    let observer = mini_with_done(&serve);
    let with_mini = |cmd: &mut Command| {
        cmd.env("FAKE_REX_OBSERVER_mini", &observer);
    };
    let mut ls = serve.cmd();
    with_mini(&mut ls);
    let first: Vec<serde_json::Value> = serde_json::from_slice(
        &ls.args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    let done = row(&first, "mini/block:web-done");
    assert_eq!(done["seen"], false);
    let quiet_since = row(&first, "mini/block:web-quiet")["since"].clone();
    let first_pid = serve.pid();
    serve
        .cmd()
        .args(["seen", "mini/block:web-done"])
        .assert()
        .success()
        .stdout("seen mini/block:web-done\n");
    serve
        .cmd()
        .args(["serve", "--stop"])
        .assert()
        .success()
        .stdout("serve stopped\n");
    let state = std::fs::read_to_string(serve.file("state.json")).unwrap();
    assert!(state.contains(r#""mini/block:web-done":{"#), "{state}");
    let mut again = serve.cmd();
    with_mini(&mut again);
    let second: Vec<serde_json::Value> = serde_json::from_slice(
        &again
            .args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_ne!(serve.pid(), first_pid, "a new serve answered");
    let done = row(&second, "mini/block:web-done");
    assert_eq!(done["seen"], true, "seen survives the restart");
    assert_eq!(done["since"], "2026-01-01T00:00:00.000Z");
    assert_eq!(
        row(&second, "mini/block:web-quiet")["since"],
        quiet_since,
        "an unchanged pane keeps its since"
    );
}

#[test]
fn serve_refuses_second_instance() {
    let serve = Serve::new();
    serve
        .cmd()
        .arg("serve")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("serve started"));
    serve
        .cmd()
        .args(["serve", "--foreground", "--json"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"SERVE_RUNNING\""));
    serve
        .cmd()
        .arg("serve")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("serve is already running"));
}

#[test]
fn seen_command_marks_row_and_subscribers_see_it() {
    let serve = Serve::new();
    let observer = mini_with_done(&serve);
    let mut envs: Vec<(&str, String)> = serve.envs();
    envs.push(("FAKE_REX_OBSERVER_mini", observer));
    let envs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let marked = std::cell::Cell::new(false);
    let seen_of = |prints: &[serde_json::Value]| -> Option<bool> {
        prints
            .last()?
            .as_array()?
            .iter()
            .find(|r| r["key"] == "mini/block:web-done")?["seen"]
            .as_bool()
    };
    let (prints, _) = watch_until(&["--json"], &envs, |prints| match seen_of(prints) {
        Some(false) if !marked.get() => {
            serve
                .cmd()
                .args(["seen", "mini/block:web-done"])
                .assert()
                .success();
            marked.set(true);
            false
        }
        Some(true) => true,
        _ => false,
    });
    assert!(marked.get());
    assert_eq!(seen_of(&prints), Some(true));
    serve
        .cmd()
        .args(["seen", "mini/block:nope", "--json"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"NOT_FOUND\""));
}

#[test]
fn serve_cleans_stale_socket() {
    let serve = Serve::new();
    std::fs::create_dir_all(serve.file("")).unwrap();
    drop(std::os::unix::net::UnixListener::bind(serve.socket()).unwrap());
    assert!(serve.socket().exists(), "a socket file nobody listens on");
    let rows = serve.rows(&[]);
    assert!(!rows.is_empty());
}

#[test]
fn serve_next_cycles_through_needs_you() {
    use std::io::{BufRead, BufReader, Write};
    let serve = Serve::new();
    let observer = mini_with_done(&serve);
    let mut ls = serve.cmd();
    ls.env("FAKE_REX_OBSERVER_mini", &observer)
        .args(["ls", "--json"])
        .assert()
        .success();
    let ask = |request: serde_json::Value| -> serde_json::Value {
        let mut stream = std::os::unix::net::UnixStream::connect(serve.socket()).unwrap();
        writeln!(stream, "{request}").unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let first = ask(serde_json::json!({ "cmd": "next" }));
    assert_eq!(first["row"]["key"], "mini/block:web-done");
    let again = ask(serde_json::json!({ "cmd": "next", "after": "mini/block:web-done" }));
    assert_eq!(
        again["row"]["key"], "mini/block:web-done",
        "one pane wraps to itself"
    );
    ask(serde_json::json!({ "cmd": "seen", "key": "mini/block:web-done" }));
    let none = ask(serde_json::json!({ "cmd": "next" }));
    assert_eq!(none["error"]["code"], "NOBODY_NEEDS_YOU");
    let bad = ask(serde_json::json!({ "cmd": "dance" }));
    assert_eq!(bad["error"]["code"], "USAGE");
}

#[test]
fn serve_restarts_a_host_that_sends_a_bad_record() {
    let serve = Serve::new();
    let bad = r#"{"v":1,"type":"status","session_id":"session:web","block_id":"block:web-seat-b","record_id":"","record":{"app":"x"}}"#;
    serve
        .cmd()
        .env("FAKE_REX_APPEND_mini", bad)
        .args(["ls", "--json"])
        .assert()
        .success();
    eventually("a second mini observer", || serve.observer_starts() >= 3);
    let log = std::fs::read_to_string(serve.file("serve.log")).unwrap();
    assert!(
        log.contains("warning: mini: unexpected pane shape from Rex 0.0.1792787366-g623b2d: missing field `state`; restarting its observer: {"),
        "{log}"
    );
    assert!(!log.contains("will not restart"), "{log}");
}

#[test]
fn serve_sweep_skips_a_pid_that_is_not_its_observer() {
    let serve = Serve::new();
    let mut bystander = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    std::fs::create_dir_all(serve.file("")).unwrap();
    let record = serde_json::json!({ "pid": 999_999, "rex": fixture("fake-rex"),
        "children": [bystander.id()] });
    std::fs::write(serve.file("serve.lock"), record.to_string()).unwrap();
    serve.rows(&[]);
    let log = serve.file("serve.log");
    eventually("the sweep line in serve.log", || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("sweep: skipped pid")
    });
    assert!(
        bystander.try_wait().unwrap().is_none(),
        "the bystander is still running"
    );
    bystander.kill().unwrap();
    bystander.wait().unwrap();
}

#[test]
fn serve_subscriber_gets_rows_as_soon_as_hosts_report() {
    let serve = Serve::new();
    let envs = serve.envs();
    let envs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let started = std::time::Instant::now();
    let (prints, _) = watch_until(&["--json"], &envs, |prints| !prints.is_empty());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the first rows must not wait for the 5s fallback, took {:?}",
        started.elapsed()
    );
    assert!(!prints[0].as_array().unwrap().is_empty());
}

#[test]
fn serve_keeps_memory_of_a_host_that_stays_silent_for_a_run() {
    let serve = Serve::new();
    let observer = mini_with_done(&serve);
    let silent = serve.dir.path().join("silent.ndjson");
    std::fs::write(&silent, "").unwrap();
    let run = |mini: &str| -> Vec<serde_json::Value> {
        let out = serve
            .cmd()
            .env("FAKE_REX_OBSERVER_mini", mini)
            .args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).unwrap()
    };
    run(&observer);
    serve
        .cmd()
        .args(["seen", "mini/block:web-done"])
        .assert()
        .success();
    serve.cmd().args(["serve", "--stop"]).assert().success();
    let quiet = run(silent.to_str().unwrap());
    assert!(
        quiet.iter().all(|row| row["host"] != "mini"),
        "mini sent nothing this run"
    );
    serve.cmd().args(["serve", "--stop"]).assert().success();
    let state = std::fs::read_to_string(serve.file("state.json")).unwrap();
    assert!(state.contains(r#""mini/block:web-done":{"#), "{state}");
    let back = run(&observer);
    assert_eq!(
        row(&back, "mini/block:web-done")["seen"],
        true,
        "seen survives a silent run"
    );
}

#[test]
fn seen_without_a_host_is_usage() {
    bin()
        .args(["seen", "block:abc", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("KEY must be HOST/BLOCK_ID"));
}

#[test]
fn ls_text_squeezes_long_labels_and_dates_only_reported_rows() {
    let out = ls().assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("web › 2-openrig:ri…89abcdef#l2 › seat-r"),
        "a long tab label is squeezed in the middle:\n{text}"
    );
    let seat = text.lines().find(|l| l.contains("seat-a@demo")).unwrap();
    assert!(
        seat.contains("unreported  idle     -  "),
        "no record, no age: {seat}"
    );
    let panes = ls_json(&mut ls());
    let review = by_block(&panes, "block:web-rig-review");
    assert_eq!(
        review["tab"], "2-openrig:rig:demo-team#0123456789abcdef#l2",
        "--json keeps the full label"
    );
    let seat = by_block(&panes, "block:api-seat-a");
    assert_eq!(
        seat["since_estimated"], true,
        "no record: since is a guess, as docs/cli.md says"
    );
}

/// Runs `rexboard board` on a fresh pty, sends `signal` (if any) once the
/// board is on the alternate screen, and returns everything it wrote plus how it exited.
fn board_under_pty(
    signal: Option<libc::c_int>,
    hang_up: bool,
) -> (String, std::process::ExitStatus) {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0, 0);
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut size,
        )
    };
    assert_eq!(opened, 0, "openpty failed");
    // Close-on-exec, so children that other tests spawn meanwhile do not inherit the pty.
    for fd in [master, slave] {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let stdio = || unsafe {
        std::process::Stdio::from_raw_fd(libc::fcntl(slave, libc::F_DUPFD_CLOEXEC, 0))
    };
    // No setsid or controlling tty: crossterm reads a tty stdin, and a pre_exec fork from the
    // busy parallel suite got the child SIGKILLed in about a third of runs.
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_rexboard"));
    cmd.arg("board")
        .env("REXBOARD_REX", fixture("fake-rex"))
        .env("XDG_RUNTIME_DIR", NO_SERVE)
        .env("XDG_STATE_HOME", NO_SERVE)
        .env("TMPDIR", NO_SERVE)
        .env("TERM", "xterm-256color")
        .stdin(stdio())
        .stdout(stdio())
        .stderr(stdio());
    let mut child = cmd.spawn().unwrap();
    unsafe { libc::close(slave) };
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let close = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sink, closing) = (seen.clone(), close.clone());
    unsafe { libc::fcntl(master, libc::F_SETFL, libc::O_NONBLOCK) };
    let reader = std::thread::spawn(move || {
        // Owns the master: dropping it on `close` hangs up the terminal, as a closed window does.
        let mut file = unsafe { std::fs::File::from_raw_fd(master) };
        let mut chunk = [0; 4096];
        while !closing.load(std::sync::atomic::Ordering::SeqCst) {
            match file.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => sink.lock().unwrap().extend_from_slice(&chunk[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    let text = || String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !text().contains("\x1b[?1049h") {
        if std::time::Instant::now() >= deadline {
            let status = child.try_wait().unwrap();
            let _ = child.kill();
            panic!(
                "no alternate screen in 10s; exit {status:?}; output {:?}",
                text()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if hang_up {
        close.store(true, std::sync::atomic::Ordering::SeqCst);
        reader.join().unwrap();
    }
    if let Some(signal) = signal {
        unsafe { libc::kill(child.id() as i32, signal) };
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("board did not exit within 5s of signal {signal:?} (hang up: {hang_up})");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    if !hang_up {
        eventually("the restore sequence", || text().contains("\x1b[?1049l"));
    }
    (text(), status)
}

#[test]
fn board_restores_the_terminal_on_sigterm_and_sighup() {
    for signal in [libc::SIGTERM, libc::SIGHUP] {
        let (out, status) = board_under_pty(Some(signal), false);
        assert!(status.success(), "signal {signal}: {status:?}");
        let entered = out.find("\x1b[?1049h").unwrap();
        let left = out.rfind("\x1b[?1049l").unwrap();
        assert!(
            left > entered,
            "the alternate screen is left after it was entered"
        );
        assert!(
            out[left..].contains("\x1b[?25h"),
            "the cursor is shown again"
        );
    }
}

#[test]
fn ls_does_not_suspect_a_seat_under_tmux_by_its_title() {
    // tmux owns the title and keeps the idle glyph while the agent works.
    let panes = ls_json(&mut ls());
    let seat = by_block(&panes, "block:web-rig-review");
    assert_eq!(seat["state"], "working");
    assert_eq!(seat["suspect"], false, "{seat}");
    assert_eq!(seat["group"], "working");
    let out = ls().assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    let line = text.lines().find(|l| l.contains("seat-r")).unwrap();
    assert!(!line.contains("idle?"), "{line}");
}

#[test]
fn board_exits_when_its_terminal_hangs_up() {
    let (_, status) = board_under_pty(Some(libc::SIGHUP), true);
    assert!(status.success(), "{status:?}");
}

#[test]
fn board_exits_when_its_terminal_closes_without_a_sighup() {
    let (_, status) = board_under_pty(None, true);
    assert!(status.success(), "{status:?}");
}

/// A mini observer stream whose snapshot holds two blocked panes (the older first) and an
/// unseen done pane, in two tabs of one session.
fn mini_needing(serve: &Serve) {
    let blocked = |id: &str, at: &str| {
        serde_json::json!({ "block_id": id, "label": id.trim_start_matches("block:"),
        "title": "", "process": { "name": "2.1.0" },
        "records": [{ "state": "blocked", "kind": "permission", "app": "claude-code",
                      "msg": "Allow Bash?", "updated_at": at }] })
    };
    let snapshot = serde_json::json!({ "v": 1, "type": "snapshot", "rex_version": "0.0.1-test",
        "sessions": [{ "session_id": "session:web", "label": "web", "windows": [
            { "window_id": "window:web-a", "label": "build", "blocks": [
                blocked("block:seat-old", "2026-01-01T00:00:00Z"),
                blocked("block:seat-new", "2026-01-01T00:05:00Z") ] },
            { "window_id": "window:web-b", "label": "docs", "blocks": [
                { "block_id": "block:seat-done", "label": "seat-done", "title": "", "process": {},
                  "records": [{ "state": "done", "app": "claude-code", "msg": "Docs pass",
                                "updated_at": "2026-01-01T00:10:00Z" }] } ] } ] }] });
    let path = serve.dir.path().join("observer-mini-needing.ndjson");
    std::fs::write(path, format!("{snapshot}\n")).unwrap();
}

/// A serve whose local host is quiet and whose mini needs you three times, once mini's
/// snapshot is in.
fn needing() -> Serve {
    let serve = Serve::new();
    std::fs::write(
        serve.dir.path().join("observer-studio-quiet.ndjson"),
        "{\"v\":1,\"type\":\"snapshot\",\"rex_version\":\"0.0.1-test\",\"sessions\":[]}\n",
    )
    .unwrap();
    mini_needing(&serve);
    eventually("mini's rows", || serve.needing_rows().len() == 3);
    serve
}

impl Serve {
    /// A command with the `needing()` observers, outside any pane.
    fn needing_cmd(&self) -> Command {
        let mut cmd = self.cmd();
        cmd.env(
            "FAKE_REX_OBSERVER_studio",
            self.dir.path().join("observer-studio-quiet.ndjson"),
        )
        .env(
            "FAKE_REX_OBSERVER_mini",
            self.dir.path().join("observer-mini-needing.ndjson"),
        )
        .env_remove("REX_BLOCK")
        .env_remove("REX_CLIENT");
        cmd
    }

    fn next(&self, args: &[&str], from: Option<&str>) -> String {
        let mut cmd = self.needing_cmd();
        if let Some(block) = from {
            cmd.env("REX_BLOCK", block);
        }
        let out = cmd
            .args(["--json", "next"])
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let reply: serde_json::Value = serde_json::from_slice(&out).unwrap();
        reply["key"].as_str().unwrap().to_owned()
    }

    fn needing_rows(&self) -> Vec<serde_json::Value> {
        let out = self
            .needing_cmd()
            .args(["ls", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).unwrap()
    }
}

#[test]
fn jump_builds_remote_select_for_remote_host() {
    let serve = needing();
    let out = serve
        .needing_cmd()
        .args(["jump", "mini/block:seat-new"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "jumped to web › build › seat-new on mini\n"
    );
    let jumps = serve.jumps();
    assert_eq!(jumps.len(), 1, "{jumps:?}");
    assert_eq!(
        jumps[0],
        serde_json::json!({ "host": "mini", "session_id": "session:web",
            "window_id": "window:web-a", "block_id": "block:seat-new", "app_client": null })
    );
}

#[test]
fn jump_ignores_a_pre_cmd_an_old_config_still_has() {
    let serve = needing();
    let log = serve.log.to_str().unwrap().to_owned();
    serve.config(&format!(
        "client = \"main-app\"\n[jump]\npre_cmd = [\"sh\", \"-c\", \"echo pre-cmd >> {log}\"]\n"
    ));
    serve.needing_cmd().args(["jump", "1"]).assert().success();
    assert!(
        !serve.log_lines().iter().any(|l| l == "pre-cmd"),
        "pre_cmd no longer runs"
    );
    assert_eq!(
        serve.jumps()[0]["block_id"],
        "block:seat-old",
        "1 is the first NEEDS YOU row"
    );
    assert_eq!(
        serve.jumps()[0]["app_client"],
        "main-app",
        "the client from config.toml"
    );
}

#[test]
fn jump_marks_seen() {
    let serve = needing();
    let before = serve.needing_rows();
    assert_eq!(row(&before, "mini/block:seat-done")["group"], "needs_you");
    serve
        .needing_cmd()
        .args(["jump", "mini/block:seat-done"])
        .assert()
        .success();
    let after = serve.needing_rows();
    assert_eq!(row(&after, "mini/block:seat-done")["seen"], true);
    assert_eq!(row(&after, "mini/block:seat-done")["group"], "quiet");
    serve
        .needing_cmd()
        .args(["--json", "jump", "mini/block:nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"NOT_FOUND\""));
}

#[test]
fn jump_ambiguous_client_lists_labels() {
    let serve = needing();
    serve
        .needing_cmd()
        .env(
            "FAKE_REX_JUMP_REPLY",
            r#"{"ok":false,"code":"ambiguous_client","labels":["studio-app","spare-app"]}"#,
        )
        .args(["--json", "jump", "1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"AMBIGUOUS_CLIENT\""))
        .stderr(predicate::str::contains("studio-app"))
        .stderr(predicate::str::contains("spare-app"));
    assert_eq!(
        row(&serve.needing_rows(), "mini/block:seat-old")["seen"],
        false,
        "a jump that did not land marks nothing seen"
    );
}

#[test]
fn next_cycles_needs_you_in_order_and_wraps() {
    let serve = needing();
    assert_eq!(
        serve.next(&[], None),
        "mini/block:seat-old",
        "the longest waiting first"
    );
    assert_eq!(
        serve.next(&[], Some("block:seat-old")),
        "mini/block:seat-new"
    );
    assert_eq!(
        serve.next(&[], Some("block:seat-new")),
        "mini/block:seat-done"
    );
    assert_eq!(
        serve.next(&[], Some("block:seat-done")),
        "mini/block:seat-old",
        "all seen: the blocked panes still need you, from the top"
    );
    assert_eq!(
        serve.next(&["--back"], Some("block:seat-old")),
        "mini/block:seat-new"
    );
}

/// Every line in a FAKE_REX_ENV_LOG, which must exist and show no caller target.
fn assert_no_rex_targets(env_log: &std::path::Path) {
    let lines = std::fs::read_to_string(env_log).unwrap();
    assert!(!lines.is_empty(), "rex was not called");
    for line in lines.lines() {
        assert_eq!(line, "REX_BLOCK=- REX_SESSION=- REX_CLIENT=-");
    }
}

#[test]
fn next_keeps_the_callers_rex_targets_out_of_rex() {
    let serve = needing();
    let env_log = serve.dir.path().join("rex-env.log");
    let out = serve
        .needing_cmd()
        .env("REX_BLOCK", "block:seat-old")
        .env("REX_SESSION", "session:caller")
        .env("FAKE_REX_ENV_LOG", &env_log)
        .args(["--json", "next"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reply: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        reply["key"], "mini/block:seat-new",
        "REX_BLOCK is still the cursor"
    );
    assert_no_rex_targets(&env_log);
}

#[test]
fn next_skips_seen_done() {
    let serve = needing();
    serve
        .needing_cmd()
        .args(["seen", "mini/block:seat-done"])
        .assert()
        .success();
    assert_eq!(
        serve.next(&[], Some("block:seat-new")),
        "mini/block:seat-old"
    );
}

#[test]
fn next_with_nobody_waiting_says_so() {
    let serve = Serve::new();
    let observer = mini_with_done(&serve);
    let cmd = || {
        let mut cmd = serve.cmd();
        cmd.env("FAKE_REX_OBSERVER_mini", &observer)
            .env_remove("REX_BLOCK");
        cmd
    };
    eventually("mini's done row", || {
        let out = cmd().args(["ls", "--json"]).output().unwrap().stdout;
        String::from_utf8_lossy(&out).contains("block:web-done")
    });
    cmd()
        .args(["seen", "mini/block:web-done"])
        .assert()
        .success();
    cmd()
        .arg("next")
        .assert()
        .code(1)
        .stderr("nobody needs you\n");
    cmd()
        .args(["--json", "next"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"NOBODY_NEEDS_YOU\""));
}

#[test]
fn config_excludes_hosts_and_rejects_unknown_keys() {
    let serve = Serve::new();
    serve.config("[hosts]\nexclude = [\"mini\"]\n");
    let rows = serve.rows(&[]);
    assert!(rows.iter().all(|r| r["host"] == "local"), "{rows:?}");
    serve
        .cmd()
        .args(["ls", "--host", "mini"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no host named mini"));
    serve.config("colour = \"red\"\n");
    serve
        .cmd()
        .args(["--json", "ls", "--direct"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("\"code\":\"CONFIG\""))
        .stderr(predicate::str::contains("colour"));
}

#[test]
fn jump_explains_an_app_that_does_not_answer() {
    let serve = needing();
    let raw = r#"stream response \"control-2\" failed: \"studio\" did not answer within 5s (status=504 code=timeout)"#;
    let reply = format!(r#"{{"ok":false,"step":"session.select","error":"{raw}"}}"#);
    serve
        .needing_cmd()
        .env("FAKE_REX_JUMP_REPLY", &reply)
        .args(["jump", "1"])
        .assert()
        .code(1)
        .stderr(
            "error: the Rex app on studio did not answer within 5s; it does not answer while \
             the Mac's screen is locked\nnext: unlock the Mac, then rexboard jump mini/block:seat-old\n",
        );
    let out = serve
        .needing_cmd()
        .env("FAKE_REX_JUMP_REPLY", &reply)
        .args(["--json", "jump", "1"])
        .assert()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let error: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(error["error"]["code"], "REX");
    assert!(
        error["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("code=timeout"),
        "--json keeps Rex's words: {error}"
    );
    assert_eq!(error["error"]["next_steps"][1], "rex client ls");
}

#[test]
fn jump_without_a_usable_app_says_which_case() {
    let serve = needing();
    let fail = |reply: &str| {
        let out = serve
            .needing_cmd()
            .env("FAKE_REX_JUMP_REPLY", reply)
            .args(["--json", "jump", "1"])
            .assert()
            .code(1)
            .get_output()
            .stderr
            .clone();
        serde_json::from_slice::<serde_json::Value>(&out).unwrap()["error"].clone()
    };
    let none = fail(r#"{"ok":false,"code":"no_app"}"#);
    assert_eq!(none["code"], "AMBIGUOUS_CLIENT");
    assert_eq!(
        none["message"],
        "no Rex app is connected to this host's Rex server"
    );
    assert_eq!(
        none["next_steps"],
        serde_json::json!(["open the Rex app", "rex client ls"])
    );
    let elsewhere = fail(r#"{"ok":false,"code":"no_unix_app","labels":["remote-app"]}"#);
    assert_eq!(
        elsewhere["message"],
        "no Rex app is on this host's unix socket; app clients: remote-app"
    );
    assert_eq!(
        elsewhere["next_steps"][0],
        "client = \"remote-app\" in config.toml, or REX_CLIENT=remote-app"
    );
}

#[test]
fn next_outside_a_pane_starts_after_the_last_jump() {
    let serve = needing();
    for key in ["seat-old", "seat-new", "seat-done"] {
        serve
            .needing_cmd()
            .args(["seen", &format!("mini/block:{key}")])
            .assert()
            .success();
    }
    // Every row is seen, so the two blocked ones cycle; only memory of the last jump moves on.
    assert_eq!(serve.next(&[], None), "mini/block:seat-old");
    assert_eq!(serve.next(&[], None), "mini/block:seat-new");
    assert_eq!(serve.next(&[], None), "mini/block:seat-old", "and wraps");
    serve
        .needing_cmd()
        .args(["jump", "mini/block:seat-new"])
        .assert()
        .success();
    assert_eq!(
        serve.next(&[], None),
        "mini/block:seat-old",
        "a jump by key moves the start too"
    );
    serve
        .needing_cmd()
        .args(["seen", "mini/block:seat-old"])
        .assert()
        .success();
    assert_eq!(
        serve.next(&[], None),
        "mini/block:seat-new",
        "seen alone does not move it"
    );
}
