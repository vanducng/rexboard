use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("rexboard").expect("binary")
}

#[test]
fn version_prints_name() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout("rexboard 0.0.0\n");
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
    cmd.env("REXBOARD_REX", fixture("fake-rex")).arg("ls");
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
            "block:api-seat-a"
        ]
    );
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
    assert!(text.contains("api › main › shell"), "{text}");
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
        .args(["ls", "--json"])
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
    assert!(panes.iter().all(|p| p.as_object().unwrap().len() == 22));
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
        &["--json", "--host", "mini"],
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
fn watch_json_keeps_other_hosts_when_one_sends_a_bad_record() {
    let bad = r#"{"v":1,"type":"status","session_id":"session:web","block_id":"block:web-seat-b","record_id":"","record":{"app":"x"}}"#;
    let marker = seat_done("session:api", "block:api-seat-a");
    let (prints, errors) = watch_until(
        &["--json"],
        &[
            ("FAKE_REX_APPEND_mini", bad),
            ("FAKE_REX_APPEND_studio", &marker),
        ],
        |prints| {
            rows_where(prints, "block:api-seat-a", "done")
                && prints
                    .last()
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["host"] == "mini" && row["stale"] == true)
        },
    );
    let last = prints.last().unwrap().as_array().unwrap();
    let mini = last.iter().find(|row| row["host"] == "mini").unwrap();
    assert_eq!(
        mini["group"], "unreported",
        "the stopped host's rows stay, marked stale"
    );
    assert!(
        errors.contains(
            "warning: mini stopped and will not restart: unexpected pane shape from Rex 0.0.1792787366-g623b2d: missing field `state`"
        ),
        "{errors}"
    );
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
        .stdout(predicate::str::is_match(r"\nworking +working +≥\d+d +codex").unwrap());
}
