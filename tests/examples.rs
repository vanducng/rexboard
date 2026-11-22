//! The settings in `examples/rexboard/` are ones rexboard reads, not text that only looks right.
use assert_cmd::Command;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `ls --direct --json` over the recorded two-host snapshot, with `config.toml` read from the
/// example. A key or rule rexboard does not take would be a `CONFIG` error, or a warning on stderr.
fn ls_with_example_config() -> (Vec<serde_json::Value>, String) {
    let config = tempfile::tempdir().unwrap();
    let dir = config.path().join("rexboard");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        root().join("examples/rexboard/config.toml"),
        dir.join("config.toml"),
    )
    .unwrap();
    let out = Command::cargo_bin("rexboard")
        .unwrap()
        .env("XDG_RUNTIME_DIR", "/nonexistent/rexboard-tests")
        .env("XDG_STATE_HOME", "/nonexistent/rexboard-tests")
        .env("XDG_CONFIG_HOME", config.path())
        .env("REXBOARD_REX", root().join("tests/fixtures/fake-rex"))
        .args(["ls", "--direct", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    (
        serde_json::from_slice(&out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn the_example_config_loads_and_its_seat_rule_marks_the_seat_panes() {
    let (rows, stderr) = ls_with_example_config();
    assert_eq!(stderr, "", "no warning about config.toml");
    let marked = |block: &str| {
        let row = rows.iter().find(|row| row["block_id"] == block).unwrap();
        (row["marked"].clone(), row["marked_by"].clone())
    };
    assert_eq!(
        marked("block:api-seat-a"),
        (true.into(), serde_json::json!(["config"]))
    );
    assert_eq!(
        marked("block:web-seat-b"),
        (true.into(), serde_json::json!(["config"]))
    );
    assert_eq!(
        marked("block:api-claude"),
        (false.into(), serde_json::json!([]))
    );
}

fn example_tui_json() -> serde_json::Map<String, serde_json::Value> {
    let text = std::fs::read_to_string(root().join("examples/rexboard/tui.json")).unwrap();
    serde_json::from_str(&text).expect("tui.json is JSON, with no comments")
}

/// The board reads `tui.json` without saying what it ignored, so the example is held to the
/// keys and values `docs/tui.md` lists.
#[test]
fn the_example_tui_json_sets_only_what_the_docs_list() {
    let doc = std::fs::read_to_string(root().join("docs/tui.md")).unwrap();
    let json = example_tui_json();
    assert_eq!(json["digits"], "jump");
    for (key, value) in &json {
        assert!(
            doc.contains(&format!("| `{key}` |")),
            "{key} is not in docs/tui.md"
        );
        assert!(
            doc.contains(&format!("{value}")),
            "{key}: {value} is not a value docs/tui.md lists"
        );
    }
}

#[test]
fn every_example_is_listed_in_the_examples_readme() {
    let dir = root().join("examples");
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
    for path in [
        "rex/rexboard.lua",
        "rexboard/config.toml",
        "rexboard/tui.json",
        "extensions/session-prefix",
    ] {
        assert!(Path::new(&dir.join(path)).exists(), "{path}");
        assert!(
            readme.contains(path),
            "examples/README.md does not name {path}"
        );
    }
}
