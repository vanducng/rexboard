//! `rexboard hook` and `rexboard hooks`, run as the agents' hooks run them: a fake `tmux` stands in
//! for the server, and a fake home holds each harness's settings.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use assert_cmd::Command;
use serde_json::{Value, json};

fn rexboard(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rexboard").expect("binary");
    for name in [
        "TMUX",
        "TMUX_PANE",
        "REX_BLOCK",
        "TERM",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("PI_CODING_AGENT_DIR", home.join(".pi/agent"));
    cmd
}

/// A `tmux` that says the pane's tty is `tty` and logs every other call to `calls`.
fn fake_tmux(dir: &Path) -> String {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in display-message) echo {tty};; *) echo \"$@\" >> {calls};; esac\n",
        tty = dir.join("tty").display(),
        calls = dir.join("calls").display(),
    );
    std::fs::write(bin.join("tmux"), script).unwrap();
    std::fs::set_permissions(bin.join("tmux"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("tty"), "").unwrap();
    format!("{}:/usr/bin:/bin", bin.display())
}

fn corpus(app: &str, event: &str) -> Value {
    include_str!("fixtures/hooks/corpus.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|row| {
            row["app"] == app
                && row["payload"]["hook_event_name"] == event
                && row["status"].is_array()
        })
        .unwrap()
}

#[test]
fn a_hook_in_tmux_writes_the_bridges_bytes_to_its_pane_and_keeps_them_there() {
    let home = tempfile::tempdir().unwrap();
    let path = fake_tmux(home.path());
    let row = corpus("claude-code", "Stop");
    rexboard(home.path())
        .env("PATH", &path)
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .args(["hook", "claude"])
        .write_stdin(row["payload"].to_string())
        .assert()
        .success()
        .stdout("");
    let written = std::fs::read_to_string(home.path().join("tty")).unwrap();
    assert_eq!(written, row["tmux"].as_str().unwrap());
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert_eq!(
        calls,
        format!(
            "set-option -p -t %3 @rex_status {}\n",
            row["fields"].as_str().unwrap()
        )
    );
    rexboard(home.path())
        .env("PATH", &path)
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .args(["hook", "claude", "SessionEnd"])
        .write_stdin("{}")
        .assert()
        .success();
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert!(
        calls.ends_with("set-option -p -u -t %3 @rex_status\n"),
        "EVENT names an event the JSON does not, and the end clears it: {calls}"
    );
}

#[test]
fn a_hook_outside_tmux_and_rex_writes_nothing_and_never_fails() {
    let home = tempfile::tempdir().unwrap();
    let path = fake_tmux(home.path());
    for stdin in [r#"{"hook_event_name":"Stop"}"#, "not json", ""] {
        rexboard(home.path())
            .env("PATH", &path)
            .args(["hook", "codex"])
            .write_stdin(stdin)
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
    assert!(!home.path().join("calls").exists());
    for args in [
        &["hook", "nope"][..],
        &["hook"],
        &["hook", "claude", "Stop", "extra"],
        &["usage"],
        &["usage", "nope", "--flag-from-later"],
    ] {
        rexboard(home.path())
            .args(args)
            .write_stdin("{}")
            .assert()
            .success();
    }
    rexboard(home.path())
        .args([
            "usage",
            "--from-a-later-build",
            "claude",
            "--",
            "sh",
            "-c",
            "cat; exit 3",
        ])
        .write_stdin("theirs")
        .assert()
        .code(3)
        .stdout("theirs");
}

#[test]
fn codexs_stop_hook_keeps_the_turns_usage_too() {
    let home = tempfile::tempdir().unwrap();
    let rollout = format!(
        "{}/tests/fixtures/harness/usage/codex-rollout.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let stop =
        json!({ "hook_event_name": "Stop", "model": "gpt-5.5-codex", "transcript_path": rollout });
    rexboard(home.path())
        .env("REX_BLOCK", "block:01a1")
        .args(["hook", "codex", "Stop"])
        .write_stdin(stop.to_string())
        .assert()
        .success();
    let kept: Value = serde_json::from_str(
        &std::fs::read_to_string(home.path().join("state/rexboard/usage/block-01a1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(kept["ctx_pct"], 4);
    assert_eq!(kept["harness"], "codex");
}

fn settings(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(home.join(".claude/settings.json")).unwrap())
        .unwrap()
}

fn backups(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".bak-")
        })
        .count()
}

#[test]
fn install_merges_each_harness_after_a_backup_and_a_second_install_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
    std::fs::write(home.join(".pi/agent/extensions/rex-status.js"), "// old").unwrap();
    let bridge =
        r#"/bin/sh -c 'exec /usr/bin/python3 "$HOME/.local/bin/rex-agent-status" claude-code'"#;
    let before = json!({
        "model": "opus",
        "hooks": { "Stop": [{ "hooks": [
            { "type": "command", "command": bridge, "timeout": 5 },
            { "type": "command", "command": "afplay done.aiff" },
        ]}]},
        "statusLine": { "type": "command", "command": "\"$HOME/.local/bin/agent-statusline\"" },
    });
    let text = serde_json::to_string_pretty(&before).unwrap() + "\n";
    std::fs::write(home.join(".claude/settings.json"), &text).unwrap();
    std::fs::set_permissions(
        home.join(".claude/settings.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let out = rexboard(home)
        .args(["hooks", "install", "--dry-run", "--json"])
        .output()
        .unwrap();
    let planned: Value = serde_json::from_slice(&out.stdout).unwrap();
    let states: Vec<&str> = planned["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["changes", "changes", "changes", "not managed"]);
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        text,
        "a dry run writes nothing"
    );

    rexboard(home).args(["hooks", "install"]).assert().success();
    let after = settings(home);
    let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["model", "hooks", "statusLine"], "its key order");
    let stop: Vec<&str> = after["hooks"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        stop[0].ends_with(" hook claude Stop || true'") && stop[1] == "afplay done.aiff",
        "{stop:?}"
    );
    assert!(
        after["statusLine"]["command"]
            .as_str()
            .unwrap()
            .contains("usage claude -- /bin/sh -c '\"$HOME/.local/bin/agent-statusline\"'"),
        "{}",
        after["statusLine"]
    );
    let mode = std::fs::metadata(home.join(".claude/settings.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "the file keeps its mode");
    assert_eq!(backups(&home.join(".claude")), 1);
    let codex: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join(".codex/hooks.json")).unwrap())
            .unwrap();
    assert_eq!(
        codex["hooks"].as_object().unwrap().len(),
        6,
        "a new hooks.json"
    );
    let pi = std::fs::read_to_string(home.join(".pi/agent/extensions/rexboard.js")).unwrap();
    assert!(
        !pi.contains("__REXBOARD__") && pi.contains("\"usage\", \"pi\""),
        "{pi}"
    );

    let out = rexboard(home)
        .args(["hooks", "install", "--json"])
        .output()
        .unwrap();
    let again: Value = serde_json::from_slice(&out.stdout).unwrap();
    let states: Vec<&str> = again["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["installed", "installed", "installed", "not managed"]
    );
    assert_eq!(backups(&home.join(".claude")), 1, "no second backup");
    assert!(
        again["harnesses"][2]["note"]
            .as_str()
            .unwrap()
            .contains("rex-status.js"),
        "pi's old extension would report twice"
    );
}

#[test]
fn install_leaves_a_linked_or_unreadable_settings_file_alone() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join("dotfiles-settings.json"), "{}").unwrap();
    std::os::unix::fs::symlink(
        home.join("dotfiles-settings.json"),
        home.join(".claude/settings.json"),
    )
    .unwrap();
    let out = rexboard(home)
        .args(["hooks", "status", "--json"])
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["harnesses"][0]["state"], "symlink");
    assert_eq!(status["harnesses"][1]["state"], "absent", "no ~/.codex");
    std::fs::remove_file(home.join(".claude/settings.json")).unwrap();
    std::fs::write(home.join(".claude/settings.json"), "{ not json").unwrap();
    rexboard(home)
        .args(["hooks", "install", "claude"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("is not valid JSON"));
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        "{ not json"
    );
}

/// Every file under `dir`, with its text, so a test can see nothing was written there.
fn tree(dir: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                tree(&path)
            } else {
                vec![(
                    path.display().to_string(),
                    std::fs::read_to_string(&path).unwrap(),
                )]
            }
        })
        .collect();
    files.sort();
    files
}

#[test]
fn install_refuses_a_linked_pi_extension_and_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let checkout = home.join("dotfiles");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
    std::fs::write(checkout.join("rexboard.js"), "// the checkout's copy\n").unwrap();
    let link = home.join(".pi/agent/extensions/rexboard.js");
    std::os::unix::fs::symlink(checkout.join("rexboard.js"), &link).unwrap();
    let before = tree(home);
    for args in [
        &["hooks", "status", "--json"][..],
        &["hooks", "install", "pi", "--dry-run", "--json"],
        &["hooks", "install", "pi", "--json"],
    ] {
        let out = rexboard(home).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}");
        let plans: Value = serde_json::from_slice(&out.stdout).unwrap();
        let pi = plans["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["harness"] == "pi")
            .unwrap();
        assert_eq!(pi["state"], "symlink", "{args:?}: {pi}");
        assert_eq!(
            pi["note"],
            "~/.pi/agent/extensions/rexboard.js is a link, as a dotfiles checkout makes it: \
             change it there"
        );
    }
    assert!(link.is_symlink(), "the link is kept");
    assert_eq!(tree(home), before, "nothing written, no backup");
}

#[test]
fn install_refuses_a_linked_harness_directory_and_writes_nothing_through_it() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let checkout = home.join("dotfiles");
    std::fs::create_dir_all(checkout.join("claude")).unwrap();
    std::fs::write(checkout.join("claude/settings.json"), "{}\n").unwrap();
    std::fs::create_dir_all(checkout.join("pi/agent/extensions")).unwrap();
    std::os::unix::fs::symlink(checkout.join("claude"), home.join(".claude")).unwrap();
    // As stow folds a package: the link sits above the harness's own directory.
    std::os::unix::fs::symlink(checkout.join("pi"), home.join(".pi")).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let before = tree(&checkout);
    let out = rexboard(home)
        .args(["hooks", "status", "--json"])
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    let plans = status["harnesses"].as_array().unwrap();
    let by = |name: &str| plans.iter().find(|p| p["harness"] == name).unwrap().clone();
    assert_eq!(by("claude")["state"], "symlink");
    assert!(
        by("claude")["note"]
            .as_str()
            .unwrap()
            .starts_with("~/.claude is a link"),
        "status names the link: {}",
        by("claude")
    );
    assert_eq!(by("pi")["state"], "symlink");
    assert!(
        by("pi")["note"]
            .as_str()
            .unwrap()
            .starts_with("~/.pi is a link")
    );
    assert_eq!(
        by("codex")["state"],
        "changes",
        "a real directory still installs"
    );
    let text = rexboard(home).args(["hooks", "install"]).output().unwrap();
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains("claude symlink") && text.contains("~/.claude is a link"),
        "{text}"
    );
    assert_eq!(tree(&checkout), before, "nothing written into the checkout");
    assert!(home.join(".codex/hooks.json").exists(), "{text}");
}

#[test]
fn replay_sends_each_panes_kept_status_again() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let (one, two) = (home.path().join("tty1"), home.path().join("tty2"));
    std::fs::write(&one, "").unwrap();
    std::fs::write(&two, "").unwrap();
    let script = format!(
        "#!/bin/sh\nprintf '%s app=pi:state=done\\n%s \\n' {} {}\n",
        one.display(),
        two.display()
    );
    std::fs::write(bin.join("tmux"), script).unwrap();
    std::fs::set_permissions(bin.join("tmux"), std::fs::Permissions::from_mode(0o755)).unwrap();
    rexboard(home.path())
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .args(["hooks", "replay", "zz"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(&one).unwrap(),
        "\x1bPtmux;\x1b\x1b]7501;app=pi:state=done\x07\x1b\\"
    );
    assert_eq!(
        std::fs::read_to_string(&two).unwrap(),
        "",
        "a pane with none gets nothing"
    );
}

#[test]
fn hooks_name_rexboard_by_full_path_the_path_entry_when_it_is_this_binary() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(
        assert_cmd::cargo::cargo_bin("rexboard"),
        bin.join("rexboard"),
    )
    .unwrap();
    let first_change = |path: &str| -> String {
        let out = rexboard(home)
            .env("PATH", path)
            .args(["hooks", "install", "claude", "--dry-run", "--json"])
            .output()
            .unwrap();
        let plan: Value = serde_json::from_slice(&out.stdout).unwrap();
        plan["harnesses"][0]["changes"][0]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let linked = format!("{}/rexboard hook claude SessionStart", bin.display());
    assert!(
        first_change(&format!("{}:/usr/bin:/bin", bin.display())).contains(&linked),
        "the PATH entry, which outlives an upgrade"
    );
    let own = assert_cmd::cargo::cargo_bin("rexboard")
        .canonicalize()
        .unwrap();
    assert!(
        first_change("/usr/bin:/bin").contains(&format!("{} hook claude", own.display())),
        "not on PATH: its own path, never a bare name"
    );
}

#[test]
fn installed_hooks_and_status_line_fail_open_with_rexboard_missing_or_too_old() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".claude/settings.json"),
        r#"{"statusLine": {"type": "command", "command": "echo theirs"}}"#,
    )
    .unwrap();
    rexboard(home)
        .args(["hooks", "install", "claude"])
        .assert()
        .success();
    let after = settings(home);
    let stop = after["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned();
    let line = after["statusLine"]["command"].as_str().unwrap().to_owned();
    let me = assert_cmd::cargo::cargo_bin("rexboard")
        .canonicalize()
        .unwrap();
    let me = me.display().to_string();
    assert!(stop.contains(&me) && line.contains(&me), "{stop}\n{line}");

    // A build from before `hook` and `usage`: clap refuses both with exit 2.
    let old = home.join("old-rexboard");
    std::fs::write(
        &old,
        "#!/bin/sh\necho \"error: unrecognized subcommand '$1'\" >&2\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
    let missing = home.join("no-such-rexboard").display().to_string();
    for bin in [me.as_str(), old.to_str().unwrap(), missing.as_str()] {
        let run = |command: &str| {
            std::process::Command::new("/bin/sh")
                .args(["-c", &command.replace(&me, bin)])
                .env("HOME", home)
                .env_remove("TMUX")
                .env_remove("REX_BLOCK")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap()
        };
        let hook = run(&stop);
        assert_eq!(hook.status.code(), Some(0), "{bin}: the hook");
        let shown = run(&line);
        assert_eq!(shown.status.code(), Some(0), "{bin}: the status line");
        assert_eq!(String::from_utf8_lossy(&shown.stdout), "theirs\n", "{bin}");
    }
}

#[test]
fn a_rexboard_whose_path_has_a_space_or_a_quote_installs_once() {
    for dir in ["My Tools", "it's"] {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let bin = home.join(dir).join("rexboard");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::copy(assert_cmd::cargo::cargo_bin("rexboard"), &bin).unwrap();
        let install = || {
            let mut cmd = Command::new(&bin);
            for name in [
                "TMUX",
                "TMUX_PANE",
                "REX_BLOCK",
                "CLAUDE_CONFIG_DIR",
                "CODEX_HOME",
            ] {
                cmd.env_remove(name);
            }
            let out = cmd
                .env("HOME", home)
                .env("PATH", "/usr/bin:/bin")
                .args(["hooks", "install", "claude", "codex", "--json"])
                .output()
                .unwrap();
            let plan: Value = serde_json::from_slice(&out.stdout).unwrap();
            plan["harnesses"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h["state"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(install(), ["written", "written"], "{dir}");
        assert_eq!(
            install(),
            ["installed", "installed"],
            "{dir}: a second install"
        );
        let stop = settings(home)["hooks"]["Stop"].clone();
        assert_eq!(
            stop.as_array().unwrap().len(),
            1,
            "{dir}: one group, not two"
        );
        let command = stop[0]["hooks"][0]["command"].as_str().unwrap().to_owned();
        let ran = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .env("REXBOARD_HOOK_LOG", home.join("hook.log"))
            .env("REX_BLOCK", "block:1")
            .env_remove("TMUX")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(ran.status.code(), Some(0), "{dir}");
        let log = std::fs::read_to_string(home.join("hook.log")).unwrap();
        assert!(
            log.contains("Stop"),
            "{dir}: the copy itself ran the hook: {log}"
        );
    }
}
