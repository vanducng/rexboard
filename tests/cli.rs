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
    assert!(text.starts_with("STATE"), "{text}");
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

fn watch_lines(cmd: &mut Command) -> Vec<serde_json::Value> {
    let out = cmd.assert().success().get_output().stdout.clone();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("watch --raw prints JSON lines"))
        .collect()
}

fn watch_raw() -> Command {
    let mut cmd = bin();
    cmd.env("REXBOARD_REX", fixture("fake-rex"))
        .args(["watch", "--raw", "--for", "2"]);
    cmd
}

#[test]
fn watch_raw_merges_two_hosts() {
    let lines = watch_lines(&mut watch_raw());
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
    let lines = watch_lines(
        watch_raw()
            .args(["--host", "local"])
            .env("FAKE_REX_EXIT_AFTER", "2")
            .env("FAKE_REX_LOG", log.path()),
    );
    let types: Vec<_> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(
        types[..5],
        [
            "snapshot",
            "session_created",
            "down",
            "snapshot",
            "session_created"
        ]
    );
    assert!(
        lines
            .iter()
            .all(|line| line["host"] == "local" && line["v"] == 1)
    );
    let downs: Vec<_> = lines.iter().filter(|l| l["type"] == "down").collect();
    assert_eq!(downs[0]["retry_in_s"], 1);
    assert_eq!(downs[0]["reason"], "observer exited (exit status: 0)");
    if let Some(second) = downs.get(1) {
        assert_eq!(
            second["retry_in_s"], 2,
            "a short run must not reset the backoff"
        );
    }
    let argv = std::fs::read_to_string(log.path()).unwrap();
    assert!(
        argv.matches("do - --all --fail-fast").count() >= 2,
        "{argv}"
    );
}

#[test]
fn watch_raw_down_reason_is_observer_stderr() {
    let lines = watch_lines(
        watch_raw()
            .args(["--host", "local"])
            .env("FAKE_REX_EXIT_AFTER", "1")
            .env(
                "FAKE_REX_STDERR",
                "observer.lua:12: attempt to index a nil value",
            ),
    );
    let down = lines.iter().find(|l| l["type"] == "down").unwrap();
    assert_eq!(
        down["reason"],
        "observer exited (exit status: 1): observer.lua:12: attempt to index a nil value"
    );
}

#[test]
fn watch_raw_passes_unknown_types_through() {
    let lines = watch_lines(watch_raw().args(["--host", "local"]).env(
        "FAKE_REX_APPEND",
        r#"{"v":1,"type":"bell","session_id":"session:api"}"#,
    ));
    let bell = lines.last().unwrap();
    assert_eq!(
        *bell,
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
        .stderr(predicate::str::contains("watch needs --raw or --json"));
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
    assert_eq!(by_block(&panes, "block:api-seat-a")["group"], "no_signal");
    assert!(panes.iter().all(|p| p.as_object().unwrap().len() == 22));
}

#[test]
fn watch_json_prints_debounced_rows() {
    let out = bin()
        .env("REXBOARD_REX", fixture("fake-rex"))
        .args(["watch", "--json", "--host", "mini", "--for", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let prints: Vec<Vec<serde_json::Value>> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is a JSON array of rows"))
        .collect();
    assert!(
        (1..=2).contains(&prints.len()),
        "a burst of 14 lines prints once or twice, got {}",
        prints.len()
    );
    let last = prints.last().unwrap();
    assert_eq!(
        last.len(),
        1,
        "the probe pane closed, only the seat is left"
    );
    assert_eq!(last[0]["block_id"], "block:web-seat-b");
    assert_eq!(last[0]["group"], "no_signal");
    assert_eq!(last[0]["host"], "mini");
}
