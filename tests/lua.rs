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
