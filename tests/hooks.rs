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
        .env("FAKE_TMUX_TTY", home.join("tty"))
        .env("FAKE_TMUX_CALLS", home.join("calls"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("PI_CODING_AGENT_DIR", home.join(".pi/agent"));
    cmd
}

/// The directory holding the fake `tmux`, which every test shares and which is written once.
///
/// A test used to write its own script and run it at once. Linux refuses to exec a file while any
/// process holds it open for writing, and a child that another test thread forks while the script
/// is being written keeps that descriptor until its own exec. Then the hook's `tmux` failed with
/// ETXTBSY, and a hook fails open: it skipped the pane, so the fake tty stayed empty.
///
/// So the script is written once, by whichever test needs it first (the others wait), and takes
/// its answers from the environment each test sets (`rexboard()` sets them). Once written, the
/// only thing that can still hold it open is a child forked before the write ended, and that
/// ends by itself within moments of its exec. So the writer runs the script once and waits, up
/// to a deadline, for ETXTBSY to stop; after that no process has it open and every exec works.
fn fake_tmux_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    // The directory outlives the guard that made it, for the whole run; this removes it when the
    // process ends.
    extern "C" fn remove() {
        if let Some(dir) = DIR.get() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    DIR.get_or_init(|| {
        let dir = prepare_fake_tmux(
            Path::new(env!("CARGO_TARGET_TMPDIR")),
            std::time::Duration::from_secs(10),
            |_| {},
        )
        .unwrap();
        unsafe { libc::atexit(remove) };
        dir
    })
}

/// Writes the fake `tmux` into a new directory under `parent` and waits up to `wait` for it to
/// run, then returns the directory, which the caller now owns. When it never runs, nothing is
/// left behind. `after_write` runs between the write and the first exec, which is where a
/// forked child's copy of the write descriptor would be.
fn prepare_fake_tmux(
    parent: &Path,
    wait: std::time::Duration,
    after_write: impl FnOnce(&Path),
) -> Result<std::path::PathBuf, String> {
    // Removes the directory unless it is kept: the write, chmod and probe can all fail.
    let guard = tempfile::Builder::new()
        .prefix("fake-tmux-")
        .tempdir_in(parent)
        .map_err(|err| err.to_string())?;
    let tmux = guard.path().join("tmux");
    // `display-message` gives the pane's tty, `list-panes` the rows replay reads, and any other
    // call is logged.
    std::fs::write(
        &tmux,
        "#!/bin/sh\n\
         [ -z \"${FAKE_TMUX_SLEEP-}\" ] || sleep \"$FAKE_TMUX_SLEEP\"\n\
         case \"$1\" in\n\
         display-message) echo \"$FAKE_TMUX_TTY\" ;;\n\
         list-panes) printf '%s' \"${FAKE_TMUX_LIST-}\" ;;\n\
         *) [ -z \"${FAKE_TMUX_CALLS-}\" ] || echo \"$@\" >> \"$FAKE_TMUX_CALLS\" ;;\n\
         esac\n",
    )
    .map_err(|err| err.to_string())?;
    std::fs::set_permissions(&tmux, std::fs::Permissions::from_mode(0o755))
        .map_err(|err| err.to_string())?;
    after_write(&tmux);
    let deadline = std::time::Instant::now() + wait;
    while let Err(err) = std::process::Command::new(&tmux).arg("probe").status() {
        if err.raw_os_error() != Some(libc::ETXTBSY) || std::time::Instant::now() >= deadline {
            return Err(format!("the fake tmux does not run: {err}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Ok(guard.keep())
}

#[test]
fn a_fake_tmux_that_never_runs_leaves_no_directory_and_one_that_does_is_kept() {
    let parent = tempfile::tempdir().unwrap();
    let left = || std::fs::read_dir(parent.path()).unwrap().count();
    let wait = std::time::Duration::from_millis(150);
    // A script that cannot run (the write, chmod or exec can fail): the directory goes with the
    // guard, not at process exit.
    let err = prepare_fake_tmux(parent.path(), wait, |tmux| {
        std::fs::set_permissions(tmux, std::fs::Permissions::from_mode(0o644)).unwrap();
    })
    .unwrap_err();
    assert!(err.contains("the fake tmux does not run"), "{err}");
    assert_eq!(left(), 0, "a failed preparation left its directory");
    // Nowhere to make it: nothing is made.
    assert!(prepare_fake_tmux(&parent.path().join("nope"), wait, |_| {}).is_err());
    // One that runs is kept, with the script in it.
    let dir = prepare_fake_tmux(parent.path(), wait, |_| {}).unwrap();
    assert!(
        dir.join("tmux").exists() && left() == 1,
        "a prepared fake tmux is kept"
    );
}

/// Only Linux refuses to exec a script some process holds open for writing.
#[cfg(target_os = "linux")]
#[test]
fn a_fake_tmux_still_held_open_for_writing_is_waited_for_and_then_given_up_on() {
    let parent = tempfile::tempdir().unwrap();
    let left = || std::fs::read_dir(parent.path()).unwrap().count();
    // A writer that holds it past the wait, as a forked child's copy of the descriptor would:
    // ETXTBSY until the wait is over, then the directory goes with the guard.
    let mut holder = None;
    let err = prepare_fake_tmux(
        parent.path(),
        std::time::Duration::from_millis(150),
        |tmux| {
            holder = Some(std::fs::OpenOptions::new().write(true).open(tmux).unwrap());
        },
    )
    .unwrap_err();
    assert!(err.contains("Text file busy"), "{err}");
    assert_eq!(left(), 0, "a preparation that gave up left its directory");
    drop(holder);
    // One that lets go while the wait runs: the script then runs, and the directory is kept.
    let dir = prepare_fake_tmux(parent.path(), std::time::Duration::from_secs(10), |tmux| {
        let held = std::fs::OpenOptions::new().write(true).open(tmux).unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(held);
        });
    })
    .unwrap();
    assert!(dir.join("tmux").exists() && left() == 1);
}

/// The PATH that finds the fake `tmux`, after making `dir`'s fake tty (`dir/tty`, empty).
fn fake_tmux(dir: &Path) -> String {
    std::fs::write(dir.join("tty"), "").unwrap();
    format!("{}:/usr/bin:/bin", fake_tmux_dir().display())
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
        let note = pi["note"].as_str().unwrap();
        assert!(
            note.starts_with("~/.pi/agent/extensions/rexboard.js is a link into a checkout")
                && note.ends_with("; change it there"),
            "{args:?}: {note}"
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
fn replay_sends_each_panes_kept_status_and_usage_again() {
    let home = tempfile::tempdir().unwrap();
    let ttys: Vec<_> = (1..=3)
        .map(|n| home.path().join(format!("tty{n}")))
        .collect();
    for tty in &ttys {
        std::fs::write(tty, "").unwrap();
    }
    // One pane with both, one with only a status (a pane an older rexboard kept), one with none.
    let rows = format!(
        "{}|app=pi:state=done|id=usage:app=pi:state=done:msg=e30=\n{}|app=pi:state=idle|\n{}||\n",
        ttys[0].display(),
        ttys[1].display(),
        ttys[2].display()
    );
    rexboard(home.path())
        .env("PATH", fake_tmux(home.path()))
        .env("FAKE_TMUX_LIST", rows)
        .args(["hooks", "replay", "zz"])
        .assert()
        .success();
    let wrapped = |pairs: &str| format!("\x1bPtmux;\x1b\x1b]7501;{pairs}\x07\x1b\\");
    assert_eq!(
        std::fs::read_to_string(&ttys[0]).unwrap(),
        wrapped("app=pi:state=done") + &wrapped("id=usage:app=pi:state=done:msg=e30=")
    );
    assert_eq!(
        std::fs::read_to_string(&ttys[1]).unwrap(),
        wrapped("app=pi:state=idle"),
        "an older rexboard kept only a status"
    );
    assert_eq!(
        std::fs::read_to_string(&ttys[2]).unwrap(),
        "",
        "a pane with none gets nothing"
    );
}

/// What a tty got of the usage report: the `usage` records in it, their JSON decoded.
fn reported(tty: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(tty).unwrap();
    text.split("]7501;")
        .filter_map(|part| part.split('\x07').next())
        .filter(|pairs| pairs.starts_with("id=usage:app="))
        .map(|pairs| {
            let state = pairs
                .split(':')
                .find_map(|p| p.strip_prefix("state="))
                .unwrap();
            match pairs.split(':').find_map(|p| p.strip_prefix("msg=")) {
                Some(msg) => serde_json::from_slice(&unbase64(msg)).unwrap(),
                None => json!({ "state": state }),
            }
        })
        .collect()
}

fn unbase64(text: &str) -> Vec<u8> {
    const ABC: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0);
    for c in text.trim_end_matches('=').chars() {
        bits = bits << 6 | ABC.find(c).unwrap() as u32;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have & 0xff) as u8);
        }
    }
    out
}

/// The pane's usage file (a lock file lives beside it).
fn usage_file(home: &Path) -> std::path::PathBuf {
    std::fs::read_dir(home.join("state/rexboard/usage"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap()
}

/// A tmux pane's usage file, rewritten as if its last report had been `seconds` ago.
fn age_last_report(home: &Path, seconds: i64) {
    let file = usage_file(home);
    let mut kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    kept["sent"]["ts"] = (chrono::Utc::now() - chrono::Duration::seconds(seconds))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .into();
    std::fs::write(&file, kept.to_string()).unwrap();
}

/// Runs rexboard in a tmux pane with the fake tmux, which `fake_tmux(home)` made once before.
fn in_tmux(home: &Path, args: &[&str], stdin: String) {
    rexboard(home)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
        )
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .args(args)
        .write_stdin(stdin)
        .assert()
        .success();
}

fn status_line(ctx: u32) -> String {
    let mut payload: Value = serde_json::from_str(include_str!(
        "fixtures/harness/usage/claude-statusline.json"
    ))
    .unwrap();
    payload["context_window"]["used_percentage"] = ctx.into();
    payload.to_string()
}

#[test]
fn a_status_line_reports_usage_to_rex_only_on_a_change_and_not_oftener_than_every_5_seconds() {
    let home = tempfile::tempdir().unwrap();
    fake_tmux(home.path());
    let tty = home.path().join("tty");
    let report = |ctx| in_tmux(home.path(), &["usage", "claude"], status_line(ctx));
    report(7);
    assert_eq!(
        reported(&tty),
        [
            json!({ "v": 1, "m": "Opus 5.5", "c": 7, "w": 1_000_000, "t": 67910, "u": 0.5435, "q": 5 })
        ]
    );
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert!(
        calls
            .starts_with("set-option -p -t %3 @rex_usage id=usage:app=claude-code:state=done:msg="),
        "{calls}"
    );
    report(7);
    assert_eq!(
        reported(&tty).len(),
        1,
        "the same figures are not sent again"
    );
    report(8);
    assert_eq!(reported(&tty).len(), 1, "a change inside 5 s waits");
    age_last_report(home.path(), 6);
    report(9);
    let sent = reported(&tty);
    assert_eq!(
        (sent.len(), &sent[1]["c"]),
        (2, &json!(9)),
        "a change after 5 s goes"
    );
    // Rex gets what was sent; the local file has what the harness said last.
    report(10);
    assert_eq!(reported(&tty).len(), 2);
    let kept: Value =
        serde_json::from_slice(&std::fs::read(usage_file(home.path())).unwrap()).unwrap();
    assert_eq!(
        (&kept["ctx_pct"], &kept["sent"]["usage"]["ctx_pct"]),
        (&json!(10), &json!(9))
    );
}

#[test]
fn a_turn_end_reports_at_once_what_the_throttle_would_hold() {
    let home = tempfile::tempdir().unwrap();
    fake_tmux(home.path());
    let tty = home.path().join("tty");
    in_tmux(home.path(), &["usage", "claude"], status_line(7));
    // Codex's Stop hook is a turn end: a changed figure goes inside the 5 s.
    let rollout = format!(
        "{}/tests/fixtures/harness/usage/codex-rollout.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let stop =
        json!({ "hook_event_name": "Stop", "model": "gpt-5.5-codex", "transcript_path": rollout });
    in_tmux(home.path(), &["hook", "codex", "Stop"], stop.to_string());
    let sent = reported(&tty);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(
        (&sent[1]["m"], &sent[1]["c"]),
        (&json!("gpt-5.5-codex"), &json!(4))
    );
}

#[test]
fn session_end_leaves_the_usage_and_the_off_switch_keeps_it_local() {
    let home = tempfile::tempdir().unwrap();
    fake_tmux(home.path());
    let tty = home.path().join("tty");
    in_tmux(home.path(), &["usage", "claude"], status_line(7));
    in_tmux(home.path(), &["hook", "claude", "SessionEnd"], "{}".into());
    // A pane keeps its last figures while it lives, as the local file does: the status clears,
    // the usage record is not touched.
    assert_eq!(reported(&tty).len(), 1, "{:?}", reported(&tty));
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert!(
        calls.contains("-u -t %3 @rex_status") && !calls.contains("-u -t %3 @rex_usage"),
        "{calls}"
    );
    let quiet = tempfile::tempdir().unwrap();
    rexboard(quiet.path())
        .env("PATH", fake_tmux(quiet.path()))
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .env("REXBOARD_NO_USAGE_REPORT", "1")
        .args(["usage", "claude"])
        .write_stdin(status_line(7))
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(quiet.path().join("tty")).unwrap(),
        ""
    );
    assert!(
        usage_file(quiet.path()).exists(),
        "the local file is still kept"
    );
}

#[test]
fn a_figure_that_did_not_reach_the_pane_is_reported_again_and_a_new_session_with_the_same_ones_is_not()
 {
    let home = tempfile::tempdir().unwrap();
    let tty = home.path().join("tty");
    fake_tmux(home.path());
    let run = |tty_for_tmux: &Path, args: &[&str], stdin: String| {
        rexboard(home.path())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
            )
            .env("TMUX", "/tmp/tmux-501/seats,1234,0")
            .env("TMUX_PANE", "%3")
            .env("FAKE_TMUX_TTY", tty_for_tmux)
            .args(args)
            .write_stdin(stdin)
            .assert()
            .success();
    };
    // tmux names a tty that cannot be opened: nothing reached the pane, so nothing is marked sent.
    run(
        &home.path().join("no/such/tty"),
        &["usage", "claude"],
        status_line(7),
    );
    assert_eq!(reported(&tty).len(), 0);
    let kept: Value =
        serde_json::from_slice(&std::fs::read(usage_file(home.path())).unwrap()).unwrap();
    assert!(kept["sent"].is_null(), "{kept}");
    // The same figures once the pane can be reached: they were never delivered, so they go.
    run(&tty, &["usage", "claude"], status_line(7));
    assert_eq!(reported(&tty).len(), 1);
    // The session ends and the next one starts with the same figures: the row already shows
    // them, so nothing is sent. Its own figures go when they differ.
    run(&tty, &["hook", "claude", "SessionEnd"], "{}".into());
    run(&tty, &["usage", "claude"], status_line(7));
    assert_eq!(reported(&tty).len(), 1);
    age_last_report(home.path(), 60);
    run(&tty, &["usage", "claude"], status_line(8));
    let sent = reported(&tty);
    assert_eq!((sent.len(), &sent[1]["c"]), (2, &json!(8)), "{sent:?}");
}

/// A hook given the 5 s its agent allows, with tmux taking 1.75 s over every command.
fn slow_hook(home: &Path, args: &[&str], stdin: String) -> std::time::Duration {
    let started = std::time::Instant::now();
    rexboard(home)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
        )
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .env("FAKE_TMUX_SLEEP", "1.75")
        .timeout(std::time::Duration::from_secs(5))
        .args(args)
        .write_stdin(stdin)
        .assert()
        .success();
    started.elapsed()
}

#[test]
fn a_hook_tells_its_status_first_and_drops_its_usage_when_tmux_is_slow() {
    let home = tempfile::tempdir().unwrap();
    let tty = home.path().join("tty");
    fake_tmux(home.path());
    let rollout = format!(
        "{}/tests/fixtures/harness/usage/codex-rollout.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    // Codex's Stop carries usage too. Its status is what must land inside the hook's 5 s.
    let stop =
        json!({ "hook_event_name": "Stop", "model": "gpt-5.5-codex", "transcript_path": rollout });
    let took = slow_hook(home.path(), &["hook", "codex", "Stop"], stop.to_string());
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    let written = std::fs::read_to_string(&tty).unwrap();
    assert!(
        written.contains("app=codex:state=done"),
        "the status went: {written:?}"
    );
    assert!(
        reported(&tty).is_empty(),
        "the usage was dropped, not started late"
    );
    // SessionEnd used to clear the usage as well, and ran over: now it only clears the status.
    let took = slow_hook(home.path(), &["hook", "claude", "SessionEnd"], "{}".into());
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert!(calls.contains("-u -t %3 @rex_status"), "{calls}");
}

#[test]
fn a_slow_stop_still_keeps_the_local_usage_and_a_fast_one_still_reports() {
    let home = tempfile::tempdir().unwrap();
    let tty = home.path().join("tty");
    fake_tmux(home.path());
    let rollout = format!(
        "{}/tests/fixtures/harness/usage/codex-rollout.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let stop =
        json!({ "hook_event_name": "Stop", "model": "gpt-5.5-codex", "transcript_path": rollout });
    // Two tmux commands of 0.3 s put the status past the quick limit: the local file is still
    // written, and Rex is not told.
    let run = |sleep: &str| {
        rexboard(home.path())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
            )
            .env("TMUX", "/tmp/tmux-501/seats,1234,0")
            .env("TMUX_PANE", "%3")
            .env("FAKE_TMUX_SLEEP", sleep)
            .args(["hook", "codex", "Stop"])
            .write_stdin(stop.to_string())
            .assert()
            .success();
    };
    run("0.3");
    let kept: Value =
        serde_json::from_slice(&std::fs::read(usage_file(home.path())).unwrap()).unwrap();
    assert_eq!(kept["model"], "gpt-5.5-codex", "{kept}");
    assert!(kept["sent"].is_null(), "nothing was sent to Rex: {kept}");
    assert!(reported(&tty).is_empty());
    // A fast Stop with the same figures still reports them: they were never sent.
    run("0");
    assert_eq!(reported(&tty).len(), 1);
}

#[test]
fn a_burst_of_callbacks_reports_once_and_nothing_is_sent_that_cannot_be_tracked() {
    let home = tempfile::tempdir().unwrap();
    let tty = home.path().join("tty");
    fake_tmux(home.path());
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| in_tmux(home.path(), &["usage", "claude"], status_line(7)));
        }
    });
    assert_eq!(reported(&tty).len(), 1, "sixteen callbacks, one report");
    // A usage directory that cannot take the figures or the lock: the report is not sent.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let quiet = tempfile::tempdir().unwrap();
    fake_tmux(quiet.path());
    let dir = quiet.path().join("state/rexboard/usage");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    in_tmux(quiet.path(), &["usage", "claude"], status_line(7));
    assert_eq!(reported(&quiet.path().join("tty")).len(), 0);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A pty whose reader is not reading and whose queue is full, as a terminal that is stuck is:
/// the master and slave stay open for the test.
struct FullPty {
    _master: std::os::fd::OwnedFd,
    _slave: std::os::fd::OwnedFd,
    path: std::path::PathBuf,
}

fn full_pty() -> FullPty {
    use std::ffi::CStr;
    use std::os::fd::FromRawFd;
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0 && libc::grantpt(master) == 0 && libc::unlockpt(master) == 0);
        let path = CStr::from_ptr(libc::ptsname(master))
            .to_str()
            .unwrap()
            .to_owned();
        let slave = libc::open(
            CStr::from_ptr(libc::ptsname(master)).as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
        );
        assert!(slave >= 0);
        let chunk = [b'x'; 4096];
        let mut full = false;
        for _ in 0..4096 {
            if libc::write(slave, chunk.as_ptr().cast(), chunk.len()) < 0 {
                full = true;
                break;
            }
        }
        assert!(full, "the pty never filled");
        FullPty {
            _master: std::os::fd::OwnedFd::from_raw_fd(master),
            _slave: std::os::fd::OwnedFd::from_raw_fd(slave),
            path: path.into(),
        }
    }
}

#[test]
fn a_full_terminal_never_stalls_the_status_line_or_the_hook() {
    let home = tempfile::tempdir().unwrap();
    fake_tmux(home.path());
    let pty = full_pty();
    let started = std::time::Instant::now();
    // The status line command it wraps still runs and says what it says.
    let out = rexboard(home.path())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
        )
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .env("FAKE_TMUX_TTY", &pty.path)
        .timeout(std::time::Duration::from_secs(5))
        .args(["usage", "claude", "--", "cat"])
        .write_stdin(status_line(7))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8_lossy(&out).contains("claude-opus-5-5"),
        "the status line still runs"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    // A hook's own status goes the same way, and does not stall either.
    let started = std::time::Instant::now();
    rexboard(home.path())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fake_tmux_dir().display()),
        )
        .env("TMUX", "/tmp/tmux-501/seats,1234,0")
        .env("TMUX_PANE", "%3")
        .env("FAKE_TMUX_TTY", &pty.path)
        .timeout(std::time::Duration::from_secs(5))
        .args(["hook", "claude", "Stop"])
        .write_stdin(json!({ "hook_event_name": "Stop" }).to_string())
        .assert()
        .success();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
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
        // A hard link, not a copy. A copy is written through a file descriptor, and a child that
        // another test forks while it is open keeps it until its exec, so on Linux executing the
        // copy then fails with ETXTBSY ("Text file busy"). A link has no write descriptor. It
        // has to be on the binary's own filesystem, so its directory is made beside the binary.
        let built = assert_cmd::cargo::cargo_bin("rexboard");
        let beside = tempfile::tempdir_in(built.parent().unwrap()).unwrap();
        let bin = beside.path().join(dir).join("rexboard");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::hard_link(&built, &bin).unwrap();
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
        let link_dir = beside.path().file_name().unwrap().to_str().unwrap();
        assert!(
            command.contains(link_dir),
            "{dir}: the hook names the linked path, not the binary it links to: {command}"
        );
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

/// `tree` without the backups a run leaves beside each file.
fn kept(dir: &Path) -> Vec<(String, String)> {
    tree(dir)
        .into_iter()
        .filter(|(path, _)| !path.contains(".bak-"))
        .collect()
}

fn hooks_run(home: &Path, args: &[&str]) -> Value {
    let out = rexboard(home)
        .args(["hooks"])
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

fn state(plans: &Value, harness: &str) -> String {
    let plan = plans["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["harness"] == harness)
        .unwrap();
    plan["state"].as_str().unwrap().to_owned()
}

/// A home as a user keeps it before install: Claude settings with their own hook, a status line
/// of their own and other keys; Codex hooks of their own; pi's extensions with one of theirs.
fn lived_in(home: &Path) {
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".claude/settings.json"),
        r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "afplay done.aiff"
          }
        ]
      }
    ]
  },
  "statusLine": {
    "type": "command",
    "command": "~/bin/my line.sh --short",
    "padding": 0
  },
  "theme": "dark"
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex/hooks.json"),
        "{\"hooks\":{\"SessionStart\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"echo hi\"}]}]}}",
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
    std::fs::write(home.join(".pi/agent/extensions/mine.js"), "// mine\n").unwrap();
}

#[test]
fn uninstall_after_install_leaves_every_file_as_it_was_a_chained_status_line_included() {
    let bare = |home: &Path| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
    };
    let empty = |home: &Path| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/settings.json"), "{}").unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
    };
    let containers = |home: &Path| {
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(
            home.join(".claude/settings.json"),
            "{\"hooks\": {\"Stop\": []}, \"model\": \"opus\"}",
        )
        .unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/hooks.json"), "{\"hooks\": {}}").unwrap();
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
    };
    for setup in [lived_in as fn(&Path), bare, empty, containers] {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        setup(home);
        let before = kept(home);
        let installed = hooks_run(home, &["install"]);
        for harness in ["claude", "codex", "pi"] {
            assert_eq!(state(&installed, harness), "written", "{installed}");
        }
        assert_ne!(kept(home), before);
        let out = hooks_run(home, &["uninstall"]);
        assert_eq!(kept(home), before, "byte for byte: {out}");
        assert_eq!(
            hooks_run(home, &["uninstall"])["harnesses"][0]["state"],
            "not installed",
            "a second uninstall changes nothing"
        );
    }
}

#[test]
fn uninstall_never_brings_back_a_bridge_hook_install_replaced() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let bridge =
        "/bin/sh -c 'exec /usr/bin/python3 \"$HOME/.local/bin/rex-agent-status\" claude-code'";
    let before = json!({
        "model": "opus",
        "hooks": {
            "PreToolUse": [{ "hooks": [{ "type": "command", "command": bridge }] }],
            "Notification": [{ "hooks": [{ "type": "command", "command": "say hi" }] }]
        }
    });
    let file = home.join(".claude/settings.json");
    std::fs::write(&file, serde_json::to_string_pretty(&before).unwrap()).unwrap();
    let installed = hooks_run(home, &["install", "claude"]);
    assert!(
        installed.to_string().contains("replace `rex-agent-status`"),
        "{installed}"
    );
    hooks_run(home, &["uninstall", "claude"]);
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut want = before.clone();
    want["hooks"].as_object_mut().unwrap().remove("PreToolUse");
    assert_eq!(
        after, want,
        "everything else as it was; the bridge, which exits 2 once its script is gone, stays gone"
    );
}

#[test]
fn uninstall_keeps_what_the_user_changed_after_install() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    lived_in(home);
    hooks_run(home, &["install"]);
    let file = home.join(".claude/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    settings["hooks"]["Stop"][1]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "command", "command": "say done" }));
    settings["editor"] = json!("vim");
    settings["hooks"]["Notification"].as_array_mut().unwrap().push(json!({ "hooks": [{
        "type": "command",
        "command": "/bin/sh -c 'rexboard hook claude Notification; afplay /System/Library/Sounds/Ping.aiff'"
    }]}));
    settings["hooks"]["PreCompact"] = json!([{ "hooks": [{
        "type": "command",
        "command": "rexboard hook claude PreCompact && notify-send compacting"
    }]}]);
    std::fs::write(&file, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    let pi = home.join(".pi/agent/extensions/rexboard.js");
    std::fs::write(&pi, std::fs::read_to_string(&pi).unwrap() + "// my tweak\n").unwrap();
    let out = hooks_run(home, &["uninstall"]);
    assert_eq!(state(&out, "pi"), "not ours", "{out}");
    assert!(
        pi.exists(),
        "a changed extension is not rexboard's to remove"
    );
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let text = after.to_string();
    assert!(
        !text.contains("|| true"),
        "install's own hooks are gone: {text}"
    );
    assert_eq!(after["editor"], "vim");
    assert_eq!(after["statusLine"]["command"], "~/bin/my line.sh --short");
    let stop: Vec<&str> = after["hooks"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert_eq!(stop, ["afplay done.aiff", "say done"]);
    for theirs in [
        "afplay /System/Library/Sounds/Ping.aiff",
        "notify-send compacting",
    ] {
        assert!(
            text.contains(theirs),
            "a hook that also runs {theirs} is the user's: {text}"
        );
    }
}

#[test]
fn uninstall_on_a_home_never_installed_or_as_a_dry_run_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    lived_in(home);
    let before = tree(home);
    let out = hooks_run(home, &["uninstall"]);
    for harness in ["claude", "codex", "pi"] {
        assert_eq!(state(&out, harness), "not installed", "{out}");
    }
    assert_eq!(tree(home), before, "nothing written, no backup");
    let text = rexboard(home)
        .args(["hooks", "uninstall"])
        .output()
        .unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains("claude not installed ~/.claude/settings.json\n"),
        "{text}"
    );
    hooks_run(home, &["install"]);
    let installed = tree(home);
    let text = rexboard(home)
        .args(["hooks", "uninstall", "--dry-run"])
        .output()
        .unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains("statusLine: back to `~/bin/my line.sh --short` alone")
            && text.contains("the extension: remove it")
            && text.ends_with("dry run: nothing written; rexboard hooks uninstall writes these\n"),
        "{text}"
    );
    assert_eq!(tree(home), installed, "a dry run writes nothing");
}

#[test]
fn uninstall_refuses_a_linked_file_or_directory() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    lived_in(home);
    hooks_run(home, &["install"]);
    let checkout = home.join("dotfiles");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::rename(home.join(".claude"), checkout.join("claude")).unwrap();
    std::os::unix::fs::symlink(checkout.join("claude"), home.join(".claude")).unwrap();
    let pi = home.join(".pi/agent/extensions/rexboard.js");
    std::fs::rename(&pi, checkout.join("rexboard.js")).unwrap();
    std::os::unix::fs::symlink(checkout.join("rexboard.js"), &pi).unwrap();
    let before = tree(&checkout);
    let out = hooks_run(home, &["uninstall"]);
    assert_eq!(state(&out, "claude"), "symlink", "{out}");
    assert_eq!(state(&out, "pi"), "symlink", "{out}");
    assert_eq!(state(&out, "codex"), "written", "a real file still goes");
    assert_eq!(tree(&checkout), before, "nothing written through a link");
    assert!(pi.is_symlink());
}

#[test]
fn status_reads_a_linked_settings_file_and_says_what_it_lacks() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    lived_in(home);
    hooks_run(home, &["install", "claude"]);
    let file = home.join(".claude/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let hooks = settings["hooks"].as_object_mut().unwrap();
    hooks.remove("SessionEnd");
    hooks["Stop"] = json!([{ "hooks": [{ "type": "command", "command": "afplay done.aiff" }] }]);
    let checkout = home.join("dotfiles");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("settings.json"), settings.to_string()).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(checkout.join("settings.json"), &file).unwrap();
    let before = tree(&checkout);
    let out = hooks_run(home, &["status"]);
    let claude = &out["harnesses"][0];
    assert_eq!(claude["state"], "symlink");
    assert_eq!(
        claude["note"],
        "~/.claude/settings.json is a link into a checkout; missing: Stop, SessionEnd; change it there"
    );
    let text = rexboard(home).args(["hooks", "status"]).output().unwrap();
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("missing: Stop, SessionEnd")
    );
    assert_eq!(tree(&checkout), before, "read only");
}

#[test]
fn uninstall_never_writes_back_a_backup_an_earlier_install_made() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(
        assert_cmd::cargo::cargo_bin("rexboard"),
        bin.join("rexboard"),
    )
    .unwrap();
    let linked = format!("{}:/usr/bin:/bin", bin.display());
    let install = |path: &str| {
        let out = rexboard(home)
            .env("PATH", path)
            .args(["hooks", "install", "codex"])
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    install(&linked);
    // The first install's path goes away (an upgrade that moved the binary), so a hook that
    // points at it is stale and the second install writes its own path over it.
    std::fs::remove_file(bin.join("rexboard")).unwrap();
    install("/usr/bin:/bin");
    assert_eq!(
        backups(&home.join(".codex")),
        1,
        "the second install, on another path, backed up the first's file"
    );
    let out = rexboard(home)
        .env("PATH", "/usr/bin:/bin")
        .args(["hooks", "uninstall", "codex", "--json"])
        .output()
        .unwrap();
    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out["harnesses"][0]["state"], "removed", "{out}");
    assert!(!home.join(".codex/hooks.json").exists(), "install made it");
}

#[test]
fn status_says_the_icons_are_off_with_the_font_check_and_how_to_turn_them_on() {
    let home = tempfile::tempdir().unwrap();
    let say = |envs: &[(&str, &str)]| {
        let mut cmd = rexboard(home.path());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd.args(["hooks", "status"]).output().unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    let off = say(&[("REXBOARD_ICONS", "text")]);
    let lines: Vec<&str> = off.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("icons"))
        .expect(&off);
    let (first, second) = (lines[at], lines[at + 1]);
    assert!(first.contains("off (words)"), "{first}");
    assert!(
        second.starts_with("       printf '|\\356\\257\\222|\\n'")
            && second.contains("\"icons\": \"nerd\" in tui.json"),
        "the check sits under the label: {second}"
    );
    assert!(
        first.chars().count() <= 80 && second.chars().count() <= 80,
        "neither wraps at 80: {first} / {second}"
    );
    assert_eq!(off.lines().filter(|l| l.starts_with("icons")).count(), 1);
    let on = say(&[("REXBOARD_ICONS", "nerd")]);
    assert!(on.lines().any(|l| l == "icons  nerd marks on"), "{on}");
    let json = rexboard(home.path())
        .args(["hooks", "status", "--json"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8(json.stdout).unwrap().contains("icons"),
        "--json is unchanged"
    );
}

#[test]
fn a_hook_file_naming_rexboard_by_any_path_or_the_portable_form_is_installed_and_left_alone() {
    let portable = "/bin/sh -c 'B=$HOME/.local/bin/rexboard; [ -x \"$B\" ] || B=/opt/homebrew/bin/rexboard; \"$B\" hook codex EVENT || true'";
    let events = [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "Stop",
        "SessionEnd",
    ];
    let here = assert_cmd::cargo::cargo_bin("rexboard");
    let absolute = format!("/bin/sh -c '{} hook codex EVENT || true'", here.display());
    for (name, command) in [
        ("absolute", absolute.as_str()),
        ("bare", "/bin/sh -c 'rexboard hook codex EVENT || true'"),
        (
            "home",
            "/bin/sh -c '$HOME/.local/bin/rexboard hook codex EVENT || true'",
        ),
        ("portable", portable),
    ] {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(".codex/hooks.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let hooks: serde_json::Map<String, Value> = events
            .iter()
            .map(|event| {
                let command = command.replace("EVENT", event);
                (
                    event.to_string(),
                    json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]),
                )
            })
            .collect();
        let text = json!({ "hooks": hooks }).to_string();
        std::fs::write(&file, &text).unwrap();
        let run = |args: &[&str]| -> Value {
            let out = rexboard(home.path())
                .arg("hooks")
                .args(args)
                .arg("--json")
                .output()
                .unwrap();
            serde_json::from_slice(&out.stdout).unwrap()
        };
        let status = run(&["status"]);
        let codex = status["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "codex")
            .unwrap();
        assert_eq!(codex["state"], "installed", "{name}: {codex}");
        let install = run(&["install", "codex"]);
        assert_eq!(
            install["harnesses"][0]["state"], "installed",
            "{name}: {install}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            text,
            "{name}: left alone"
        );
        assert_eq!(backups(file.parent().unwrap()), 0, "{name}");
        let uninstall = run(&["uninstall", "codex"]);
        assert_eq!(
            uninstall["harnesses"][0]["state"], "removed",
            "{name}: {uninstall}"
        );
        assert!(!file.exists(), "{name}: every hook taken out");
    }
}

/// A Codex `hooks.json` in a new home with `stop` as the only hook of `Stop` and the right hook
/// in every other event; returns the home.
fn codex_home_with_stop(stop: &[&str]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".codex/hooks.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let own = |event: &str| format!("/bin/sh -c 'rexboard hook codex {event} || true'");
    let hooks: serde_json::Map<String, Value> = [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "Stop",
        "SessionEnd",
    ]
    .iter()
    .map(|event| {
        let commands: Vec<String> = if *event == "Stop" {
            stop.iter().map(|c| c.to_string()).collect()
        } else {
            vec![own(event)]
        };
        let list: Vec<Value> = commands
            .iter()
            .map(|c| json!({ "type": "command", "command": c, "timeout": 5 }))
            .collect();
        (event.to_string(), json!([{ "hooks": list }]))
    })
    .collect();
    std::fs::write(&file, json!({ "hooks": hooks }).to_string()).unwrap();
    home
}

fn codex_state(home: &Path) -> Value {
    let out = rexboard(home)
        .args(["hooks", "status", "--json"])
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&out.stdout).unwrap();
    status["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["harness"] == "codex")
        .unwrap()
        .clone()
}

#[test]
fn a_hook_that_cannot_report_this_event_leaves_it_missing_and_install_adds_the_right_one() {
    let portable = |event: &str| {
        format!(
            "/bin/sh -c 'B=$HOME/.local/bin/rexboard; [ -x \"$B\" ] || B=/opt/homebrew/bin/rexboard; \"$B\" hook codex {event} || true'"
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("rexboard");
    std::fs::write(&plain, "").unwrap();
    let not_executable = format!("/bin/sh -c '{} hook codex Stop || true'", plain.display());
    for (what, stop) in [
        ("another event", "/bin/sh -c 'rexboard hook codex SessionStart || true'".to_owned()),
        ("another harness", "/bin/sh -c 'rexboard hook claude Stop || true'".to_owned()),
        ("the portable form for another event", portable("SessionEnd")),
        ("a literal $HOME", "/bin/sh -c ''\\''$HOME/.local/bin/rexboard'\\'' hook codex Stop || true'".to_owned()),
        (
            "a portable script expanded early",
            r#"/bin/sh -c "B=$HOME/.local/bin/rexboard; [ -x \"$B\" ] || B=/opt/homebrew/bin/rexboard; \"$B\" hook codex Stop || true""#.to_owned(),
        ),
        ("a file that cannot be executed", not_executable),
        ("a quoted ||", "/bin/sh -c 'rexboard hook codex Stop '\\''||'\\'' true'".to_owned()),
        ("an unterminated script", "/bin/sh -c 'rexboard hook codex Stop || true".to_owned()),
        (
            "a double-quoted path with an apostrophe and $VAR",
            r#"/bin/sh -c '"/tmp/it'\''s $VAR/rexboard" hook codex Stop || true'"#.to_owned(),
        ),
    ] {
        let home = codex_home_with_stop(&[&stop]);
        assert_eq!(codex_state(home.path())["state"], "changes", "{what}");
    }
}

#[test]
fn install_never_removes_a_user_command_that_only_starts_with_rexboards() {
    let theirs = "rexboard hook codex Stop || true; printf user-work";
    let home = codex_home_with_stop(&["/bin/sh -c 'rexboard hook codex Stop || true'", theirs]);
    let file = home.path().join(".codex/hooks.json");
    let before = std::fs::read_to_string(&file).unwrap();
    rexboard(home.path())
        .args(["hooks", "install", "codex"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        before,
        "left as written"
    );
    let home = codex_home_with_stop(&[theirs]);
    rexboard(home.path())
        .args(["hooks", "install", "codex"])
        .assert()
        .success();
    let after = std::fs::read_to_string(home.path().join(".codex/hooks.json")).unwrap();
    assert!(after.contains("printf user-work"), "kept: {after}");
}

#[test]
fn uninstall_gives_back_a_status_line_wrapped_with_rexboard_spelled_with_home() {
    for bin in [
        "\"$HOME/.local/bin/rexboard\"",
        "$HOME/.local/bin/rexboard",
        "rexboard",
    ] {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(".claude/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let theirs = "echo it's \"$HOME\" costs $5";
        let quoted = format!("'{}'", theirs.replace('\'', "'\\''"));
        let command = format!(
            "{bin} usage --help >/dev/null 2>&1 && exec {bin} usage claude -- /bin/sh -c {quoted} || exec /bin/sh -c {quoted}"
        );
        std::fs::write(
            &file,
            json!({ "statusLine": { "type": "command", "command": command } }).to_string(),
        )
        .unwrap();
        rexboard(home.path())
            .args(["hooks", "uninstall", "claude"])
            .assert()
            .success();
        let after: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(after["statusLine"]["command"], theirs, "{bin}");
    }
}

#[test]
fn the_bridges_name_in_a_command_of_the_users_is_kept_and_the_bridge_itself_is_replaced() {
    let data = "printf '%s' 'rex-agent-status is retired'";
    let home = codex_home_with_stop(&[data]);
    let file = home.path().join(".codex/hooks.json");
    rexboard(home.path())
        .args(["hooks", "install", "codex"])
        .assert()
        .success();
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains("rex-agent-status is retired"),
        "kept: {after}"
    );
    let bridge = r#"/bin/sh -c '[ -n "$TMUX$REX_BLOCK" ] || case "$TERM" in tmux*|screen*) ;; *) exit 0;; esac; exec /usr/bin/python3 "$HOME/.local/bin/rex-agent-status" codex'"#;
    let home = codex_home_with_stop(&[bridge]);
    rexboard(home.path())
        .args(["hooks", "install", "codex"])
        .assert()
        .success();
    let after = std::fs::read_to_string(home.path().join(".codex/hooks.json")).unwrap();
    assert!(!after.contains("rex-agent-status"), "replaced: {after}");
}

#[test]
fn uninstall_gives_back_the_status_line_a_v0_14_install_wrote() {
    let theirs = "echo it's \"$HOME\" costs $5";
    let t = format!("'{}'", theirs.replace('\'', "'\\''"));
    for (bin, guard) in [
        ("rexboard", "command -v rexboard >/dev/null 2>&1"),
        (
            "/opt/homebrew/bin/rexboard",
            "[ -x /opt/homebrew/bin/rexboard ]",
        ),
        (
            "'/tmp/My Tools/rexboard'",
            "[ -x '/tmp/My Tools/rexboard' ]",
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(".claude/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let command =
            format!("{guard} && exec {bin} usage claude -- /bin/sh -c {t} || exec /bin/sh -c {t}");
        std::fs::write(
            &file,
            json!({ "statusLine": { "type": "command", "command": command } }).to_string(),
        )
        .unwrap();
        rexboard(home.path())
            .args(["hooks", "uninstall", "claude"])
            .assert()
            .success();
        let after: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(after["statusLine"]["command"], theirs, "{bin}");
    }
}

#[test]
fn uninstall_keeps_a_command_under_an_event_key_that_is_not_a_name_and_restores_odd_status_lines() {
    // An event key is the user's text: `Stop || true; printf user-work` is not an event.
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".codex/hooks.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let key = "Stop || true; printf user-work";
    let theirs = "rexboard hook codex Stop || true; printf user-work";
    let text =
        json!({ "hooks": { key: [{ "hooks": [{ "type": "command", "command": theirs }] }] } })
            .to_string();
    std::fs::write(&file, &text).unwrap();
    for verb in ["install", "uninstall"] {
        rexboard(home.path())
            .args(["hooks", verb, "codex"])
            .assert()
            .success();
        let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            kept["hooks"][key][0]["hooks"][0]["command"], theirs,
            "{verb}"
        );
    }
    // A Claude status line chained around an empty command, and around a path that holds the
    // words of the wrapper, comes back as it was after the hooks around it were edited.
    for (bin, theirs) in [
        ("rexboard".to_owned(), ""),
        ("'/tmp/a && exec b/rexboard'".to_owned(), "echo hi"),
        (
            "'/tmp/a usage claude -- /bin/sh -c x/rexboard'".to_owned(),
            "echo hi",
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(".claude/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let t = format!("'{theirs}'");
        let command = format!(
            "{bin} usage --help >/dev/null 2>&1 && exec {bin} usage claude -- /bin/sh -c {t} || exec /bin/sh -c {t}"
        );
        std::fs::write(
            &file,
            json!({ "padding": 1, "statusLine": { "type": "command", "command": command } })
                .to_string(),
        )
        .unwrap();
        rexboard(home.path())
            .args(["hooks", "uninstall", "claude"])
            .assert()
            .success();
        let after: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(after["statusLine"]["command"], theirs, "{bin}");
    }
}
