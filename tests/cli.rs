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
