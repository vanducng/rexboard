use std::path::Path;
use std::process::Command;

fn lua() -> &'static str {
    ["lua", "lua5.4", "luajit"]
        .into_iter()
        .find(|bin| Command::new(bin).arg("-v").output().is_ok())
        .expect("these tests run observer.lua and send.lua and need a Lua interpreter: lua, lua5.4 or luajit")
}

#[test]
fn observer_announces_a_block_once_rex_places_it() {
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
