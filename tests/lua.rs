use std::path::Path;
use std::process::Command;

fn lua() -> &'static str {
    ["lua", "lua5.4", "luajit"]
        .into_iter()
        .find(|bin| Command::new(bin).arg("-v").output().is_ok())
        .expect("these tests run observer.lua and send.lua and need a Lua interpreter: lua, lua5.4 or luajit")
}

#[test]
fn observer_announces_a_block_once_placed_and_follows_it_through_moves_and_detaches() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(lua())
        .arg(root.join("tests/fixtures/observer-harness.lua"))
        .arg(root.join("src/lua"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<_> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        lines,
        [
            "snapshot ",
            "block_created late",
            "block_created now",
            "block_closed gone",
            "session_created ",
            "block_created dup",
            "block_created boot",
            "block_created mv",
            "block_moved mv",
            "block_moved mv",
            "window_closed ",
            "block_closed mv",
            "block_created mv",
        ]
    );
}

fn answer(front: &str, title: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(lua())
        .arg(root.join("tests/fixtures/send-harness.lua"))
        .args([root.join("src/lua").to_str().unwrap(), front, title])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn answer_goes_to_a_tmux_client_and_never_to_a_wrapper_or_a_shell() {
    assert_eq!(answer("tmux", "seat-a"), "sent");
    assert_eq!(
        answer("tmux", "ask-only"),
        "refused status_changed idle → no status",
        "ask is never an answer target"
    );
    assert_eq!(
        answer("tmux", "seat-b"),
        "refused not_harness tmux",
        "another seat is on screen"
    );
    assert_eq!(
        answer("wrapper", "seat-a"),
        "refused wrapped bash",
        "OpenRig's wrapper around an attached tmux client is refused, whatever the title"
    );
    assert_eq!(
        answer("shell", "seat-a"),
        "refused not_harness zsh",
        "a shell in front is never answered"
    );
}

/// tail.lua on a pane of 5000 rows (2.2 MB) whose replies are capped: its result, "ok reads cut
/// ends-at-the-last-row bytes".
fn tail(budget: &str, cap: &str, later: Option<&str>) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(lua())
        .arg(root.join("tests/fixtures/tail-harness.lua"))
        .args([root.join("src/lua").to_str().unwrap(), budget, cap])
        .args(later)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_tail_read_on_short_replies_reaches_the_end_of_the_pane_or_fails_never_a_prefix() {
    let first =
        |result: &str| -> Vec<String> { result.split('\t').take(4).map(str::to_owned).collect() };
    // The card's 98304 bytes arrive in 2 replies of up to 64 KiB, as before.
    assert_eq!(
        first(&tail("98304", "65536", None)),
        ["true", "2", "true", "true"]
    );
    // 5000 rows at 64 KiB a reply: 35 reads, the whole pane, ending at its last row.
    assert_eq!(
        first(&tail("2457600", "65536", None)),
        ["true", "35", "false", "true"]
    );
    // 4 KiB replies would need 560 reads: the start moves up, and it is still the pane's end.
    assert_eq!(
        first(&tail("2457600", "4096", None)),
        ["true", "40", "true", "true"]
    );
    // Replies that shrink after the first: an error, not the rows up to where it gave up.
    let shrinking = tail("2457600", "65536", Some("1024"));
    assert!(
        shrinking.starts_with("false\t40\t") && shrinking.contains("not reached"),
        "{shrinking}"
    );
}

const POPUP: &str = "examples/rex/rexboard.lua";

fn popup_harness(args: &[&str]) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(lua())
        .arg(root.join("tests/fixtures/popup-harness.lua"))
        .arg(root.join(POPUP))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn popup(args: &[&str]) -> Vec<String> {
    String::from_utf8(popup_harness(args))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_rex_example_binds_ctrl_x_then_f_and_opens_one_focused_floating_layer_in_the_panes_window() {
    assert_eq!(popup(&["binds"]), ["ctrl+[KeyX]>[KeyF]\trexboard\t-"]);
    let layer = |args: &str| {
        vec![
            "window window:here".to_owned(),
            "bounds 0.08 0.08 0.84 0.84".to_owned(),
            "focus true".to_owned(),
            "flavor com.superlogical.terminal.shell".to_owned(),
            "shell none".to_owned(),
            "run sh -c".to_owned(),
            format!("args {args}"),
        ]
    };
    assert_eq!(
        popup(&["layer", "-", "tiled"]),
        layer("rexboard session:s  rexboard board --once")
    );
    // The same layer over a zoomed pane, with the pane named so the script can zoom it again.
    assert_eq!(
        popup(&["layer", "-", "zoomed"]),
        layer("rexboard session:s block:pane rexboard board --once")
    );
    for (variant, flags) in [
        ("all", ""),
        ("marked", " --status marked"),
        ("sessions", " --view session"),
        ("api", " --keep session=api"),
    ] {
        assert_eq!(
            popup(&["layer", variant, "tiled"]),
            layer(&format!("rexboard session:s  rexboard board --once{flags}")),
            "{variant}"
        );
    }
}

#[test]
fn the_rex_example_says_why_it_cannot_open() {
    assert_eq!(
        popup(&["fails", "nope", "tiled"]),
        ["rexboard: no variant named nope"]
    );
    assert_eq!(
        popup(&["fails", "-", "nowhere"]),
        ["rexboard: the pane is in no window"]
    );
    assert_eq!(popup(&["fails", "-", "refused"]), ["no room"]);
}

/// The command the example hands a terminal, run under sh with a stand-in `rexboard` that exits
/// with `code` after printing `said` on stderr, and a stand-in `rex` that logs its arguments.
/// Returns what the terminal showed, and the rex calls.
fn popup_command(view: &str, code: &str, said: &str) -> (String, String) {
    let argv = popup_harness(&["argv", "-", view]);
    let argv: Vec<&str> = std::str::from_utf8(&argv)
        .unwrap()
        .strip_suffix('\0')
        .unwrap()
        .split('\0')
        .collect();
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("rex.log");
    let script = |name: &str, body: String| {
        let path = bin.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    };
    script(
        "rexboard",
        format!("[ -z '{said}' ] || echo '{said}' >&2\nexit {code}"),
    );
    script("rex", format!("echo \"$@\" >>{}", log.display()));
    let mut run = Command::new("sh");
    run.args(&argv[1..])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.path().display()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = run.spawn().unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    (
        String::from_utf8(out.stdout).unwrap(),
        std::fs::read_to_string(&log).unwrap_or_default(),
    )
}

#[test]
fn the_rex_example_zooms_the_pane_again_unless_the_board_jumped_and_holds_an_error_for_enter() {
    let zoom =
        "api call -s session:s session.zoom_block {\"block_id\":\"block:pane\",\"zoom\":true}\n";
    // q or esc: exit 1 and nothing printed. Over a zoomed pane it is zoomed again, quietly.
    assert_eq!(
        popup_command("zoomed", "1", ""),
        (String::new(), zoom.into())
    );
    assert_eq!(
        popup_command("tiled", "1", ""),
        (String::new(), String::new())
    );
    // A jump landed: exit 0. Zooming the old pane again would pull focus back from it.
    assert_eq!(
        popup_command("zoomed", "0", ""),
        (String::new(), String::new())
    );
    // An error the board printed stays up until enter; the pane is zoomed again after it.
    assert_eq!(
        popup_command("zoomed", "1", "no client"),
        ("no client\n\npress enter to close ".into(), zoom.into())
    );
}
