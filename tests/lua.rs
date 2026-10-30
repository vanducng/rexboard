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

fn answer(front: &str, wrapper_pid: &str, title: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(lua())
        .arg(root.join("tests/fixtures/send-harness.lua"))
        .args([
            root.join("src/lua").to_str().unwrap(),
            front,
            wrapper_pid,
            title,
        ])
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
fn answer_goes_to_a_tmux_client_or_the_wrapper_rexboard_proved_holds_one() {
    assert_eq!(answer("tmux", "-", "seat-a"), "sent");
    assert_eq!(answer("wrapper", "310", "seat-a"), "sent");
    assert_eq!(
        answer("wrapper", "-", "seat-a"),
        "refused not_harness bash",
        "no proof of a tmux child: a sh wrapper of anything else"
    );
    assert_eq!(
        answer("wrapper", "999", "seat-a"),
        "refused not_harness bash",
        "the proof was for another process"
    );
    assert_eq!(
        answer("wrapper", "310", "seat-b"),
        "refused not_harness bash",
        "another seat is on screen"
    );
    assert_eq!(
        answer("shell", "320", "seat-a"),
        "refused not_harness zsh",
        "a shell in front is never answered"
    );
}
