//! The pi extension `rexboard hooks install` writes, run under node against a stand-in for pi.
use std::path::Path;
use std::process::Command;

/// What the extension reported for one run: pi's `ctx.mode` and `ctx.hasUI`, in `env`.
fn reported(mode: &str, has_ui: bool, env: &[(&str, &str)]) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut cmd = Command::new("node");
    cmd.arg(root.join("tests/fixtures/pi-extension-harness.mjs"))
        .arg(root.join("src/hook/pi-extension.js"))
        .args([mode, &has_ui.to_string()])
        .env_remove("TMUX")
        .env_remove("REX_BLOCK")
        .env("TERM", "xterm-256color")
        .envs(env.iter().copied());
    let out = cmd
        .output()
        .expect("these tests run pi-extension.js and need node");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

const IN_TMUX: &[(&str, &str)] = &[("TMUX", "/tmp/tmux-1/default,1,0"), ("TMUX_PANE", "%1")];

#[test]
fn interactive_pi_in_a_pane_reports_its_run() {
    assert_eq!(
        reported("tui", true, IN_TMUX),
        [
            "hook pi SessionStart",
            "hook pi UserPromptSubmit",
            "usage pi",
            "hook pi Stop",
            "hook pi SessionEnd"
        ]
    );
}

#[test]
fn pi_dash_p_in_a_pane_reports_and_leaves_its_result_up() {
    let said = reported("print", false, IN_TMUX);
    assert_eq!(
        said,
        [
            "hook pi SessionStart",
            "hook pi UserPromptSubmit",
            "usage pi",
            "hook pi Stop"
        ],
        "no SessionEnd: the one run's result stays on the board"
    );
}

#[test]
fn a_json_mode_run_started_by_another_program_reports_nothing_even_in_a_pane() {
    assert_eq!(reported("json", false, IN_TMUX), Vec::<String>::new());
    assert_eq!(
        reported("json", false, &[("REX_BLOCK", "block:parent")]),
        Vec::<String>::new()
    );
}

#[test]
fn outside_a_pane_nothing_reports_whatever_the_mode() {
    for (mode, ui) in [("tui", true), ("print", false), ("json", false)] {
        assert_eq!(reported(mode, ui, &[]), Vec::<String>::new(), "{mode}");
    }
}
