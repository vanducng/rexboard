//! Acts on a pane: answers it (one `rex do` with `send.lua` re-checks the pane and sends a digit
//! or a line), or closes it.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::Error;
use crate::output::clean;
use crate::pane::Pane;
use crate::rex;

const SCRIPT: &str = concat!(
    include_str!("lua/tail.lua"),
    "\n",
    include_str!("lua/send.lua")
);
/// The re-read takes as much of the pane as the card's tail does.
const BYTES: usize = 48 * 1024;
const WITHIN: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// One digit key, no Enter.
    Pick(char),
    /// A paste, then Enter.
    Text(String),
}

impl Action {
    fn args(&self) -> Value {
        match self {
            Self::Pick(digit) => json!({ "pick": digit.to_string() }),
            Self::Text(text) => json!({ "text": text }),
        }
    }
}

/// What the operator saw, which the pane must still show when the key goes out.
#[derive(Debug, Clone, PartialEq)]
pub struct Expect {
    /// What the agent was waiting on: its main record's state and kind. A new report of the same
    /// (a hook's notification for the same dialog) is no change.
    pub state: String,
    pub kind: Option<String>,
    pub fingerprint: String,
    /// For a tmux seat, the title of the seat that was on screen.
    pub title: Option<String>,
}

/// Why a seat behind OpenRig's `sh -c ... tmux attach` wrapper is not answered.
pub const WRAPPED: &str = "a seat behind OpenRig's sh wrapper around tmux cannot be answered from rexboard yet; nothing sent";

/// Sends `action` to the pane if it still shows `expect`; else why nothing was sent.
pub fn send(
    rex: &Path,
    pane: &Pane,
    expect: &Expect,
    action: &Action,
) -> std::result::Result<(), String> {
    let args = json!({
        "session_id": pane.session_id,
        "block_id": pane.block_id,
        "max_bytes": BYTES,
        "expect": {
            "state": expect.state,
            "kind": expect.kind,
            "fingerprint": expect.fingerprint,
            "title": expect.title,
        },
        "harness": crate::harness::processes(),
        "harness_prefixes": crate::harness::process_prefixes(),
        "dialog": crate::harness::dialog::SHARED,
        "action": action.args(),
    });
    let remote = (pane.host != "local").then_some(pane.host.as_str());
    let reply = match rex::run_script_with(rex, remote, SCRIPT, &args, WITHIN, &|| false) {
        Ok(reply) => reply,
        Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::TimedOut => {
            return Err(format!(
                "the pane did not answer within {}s, so it may or may not have the key; look before sending again",
                WITHIN.as_secs()
            ));
        }
        Err(err) => return Err(format!("send failed: {err}")),
    };
    if reply["ok"] == true {
        return Ok(());
    }
    let field = |name: &str| reply[name].as_str().map(clean).unwrap_or_default();
    Err(match reply["code"].as_str() {
        Some("status_changed") => format!(
            "the agent's status changed since it was read ({}); nothing sent",
            field("detail")
        ),
        Some("dialog_up") => "a dialog is up; pick a number; nothing sent".to_owned(),
        Some("not_harness") => format!(
            "{} is in front, not the agent; nothing sent",
            field("detail")
        ),
        Some("wrapped") => WRAPPED.to_owned(),
        Some("prompt_changed") => "the screen changed since it was read; nothing sent".to_owned(),
        Some("gone") => "the pane is gone; nothing sent".to_owned(),
        Some(code) => format!("{}; nothing sent", clean(code)),
        None => match field("step").as_str() {
            "enter" => "the text was pasted but Enter did not go through; it is in the pane's input line, so jump to it".to_owned(),
            "block.event" => format!("send failed at block.event: {}", field("error")),
            step => format!("send failed at {step}: {}; nothing sent", field("error")),
        },
    })
}

/// Closes the pane with `rex [-S host] block close BLOCK`.
pub fn close(rex: &Path, pane: &Pane) -> std::result::Result<(), String> {
    let mut cmd = rex::command(rex);
    if pane.host != "local" {
        cmd.args(["-S", &pane.host]);
    }
    cmd.args(["block", "close", &pane.block_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let out = rex::spawn(&mut cmd)
        .and_then(|child| child.wait_with_output())
        .map_err(|err| format!("close failed: could not run {}: {err}", rex.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let why = stderr.lines().find(|line| !line.trim().is_empty());
    Err(format!(
        "close failed: {}",
        why.map_or_else(|| out.status.to_string(), clean)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rex that logs its argv, its REX_ variables and its script to `dir`, then prints `reply`.
    fn fake_rex(dir: &Path, reply: &Value) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("reply"), reply.to_string()).unwrap();
        let rex = dir.join("rex");
        let d = dir.display();
        std::fs::write(
            &rex,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" > '{d}/argv'\nenv | grep -E '^REX_(BLOCK|SESSION|CLIENT)=' > '{d}/env'\ncat > '{d}/script'\ncat '{d}/reply'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&rex, std::fs::Permissions::from_mode(0o755)).unwrap();
        rex
    }

    fn pane(host: &str) -> Pane {
        serde_json::from_value(json!({
            "host": host, "session_id": "session:api", "session": "api",
            "window_id": "window:main", "tab": "main", "block_id": "block:seat-b",
            "pane": "seat-b", "title": "seat-b", "process": "claude", "harness": "claude-code",
            "state": "blocked", "kind": null, "msg": null,
            "updated_at": "2026-01-01T00:00:00Z", "source": "native", "children": {}
        }))
        .unwrap()
    }

    fn expect() -> Expect {
        Expect {
            state: "blocked".into(),
            kind: Some("permission".into()),
            fingerprint: " ❯ 1. Yes\n   2. No".into(),
            title: None,
        }
    }

    fn sent(dir: &Path) -> (String, Value) {
        let argv = std::fs::read_to_string(dir.join("argv")).unwrap();
        let (head, args) = argv.trim().split_once(" --args ").unwrap();
        (head.to_owned(), serde_json::from_str(args).unwrap())
    }

    #[test]
    fn answer_pick_sends_one_digit_with_what_was_seen_on_the_rows_host() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true }));
        assert_eq!(
            send(&rex, &pane("mini"), &expect(), &Action::Pick('1')),
            Ok(())
        );
        let (head, args) = sent(dir.path());
        assert_eq!(head, "-S mini do - --json");
        assert_eq!(args["action"], json!({ "pick": "1" }), "a digit, no Enter");
        assert_eq!(args["session_id"], "session:api");
        assert_eq!(args["block_id"], "block:seat-b");
        assert_eq!(args["expect"]["state"], "blocked");
        assert_eq!(args["expect"]["kind"], "permission");
        assert_eq!(args["expect"]["fingerprint"], " ❯ 1. Yes\n   2. No");
        assert_eq!(
            args["harness"],
            json!(["claude", "codex", "pi"]),
            "from the registry"
        );
        assert_eq!(args["harness_prefixes"], json!(["grok-"]));
        assert_eq!(
            args["dialog"],
            json!({
                "cursor": ["❯", ">"],
                "input": ["❯", "›", ">"],
                "separators": [".", ")"],
                "lasting": ["don't ask again", "always", "auto mode", "this session", "accept edits"],
            }),
            "the board's own grammar, so send.lua reads the screen as the board does"
        );
        let script = std::fs::read_to_string(dir.path().join("script")).unwrap();
        assert!(script.contains("function tail(") && script.contains("block.event"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("env")).unwrap(),
            "",
            "no REX_BLOCK, REX_SESSION or REX_CLIENT reaches rex"
        );
        send(&rex, &pane("local"), &expect(), &Action::Pick('1')).unwrap();
        assert!(
            sent(dir.path()).0.starts_with("do -"),
            "no -S for this host"
        );
    }

    #[test]
    fn answer_text_sends_the_line_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({ "ok": true }));
        let line = Action::Text("fix the test".into());
        send(&rex, &pane("local"), &expect(), &line).unwrap();
        assert_eq!(
            sent(dir.path()).1["action"],
            json!({ "text": "fix the test" })
        );
    }

    #[test]
    fn answer_each_refusal_says_why_and_that_nothing_was_sent() {
        let dir = tempfile::tempdir().unwrap();
        for (reply, text) in [
            (
                json!({ "code": "status_changed", "detail": "blocked/permission → working" }),
                "the agent's status changed since it was read (blocked/permission → working); nothing sent",
            ),
            (
                json!({ "code": "dialog_up" }),
                "a dialog is up; pick a number; nothing sent",
            ),
            (
                json!({ "step": "program_status", "error": "timeout" }),
                "send failed at program_status: timeout; nothing sent",
            ),
            (
                json!({ "step": "enter", "error": "bad_request" }),
                "the text was pasted but Enter did not go through; it is in the pane's input line, so jump to it",
            ),
            (
                json!({ "code": "not_harness", "detail": "zsh" }),
                "zsh is in front, not the agent; nothing sent",
            ),
            (json!({ "code": "wrapped", "detail": "bash" }), WRAPPED),
            (
                json!({ "code": "prompt_changed" }),
                "the screen changed since it was read; nothing sent",
            ),
            (
                json!({ "code": "gone", "detail": "not_found" }),
                "the pane is gone; nothing sent",
            ),
            (
                json!({ "step": "block.event", "error": "bad_request" }),
                "send failed at block.event: bad_request",
            ),
        ] {
            let mut reply = reply;
            reply["ok"] = json!(false);
            let rex = fake_rex(dir.path(), &reply);
            assert_eq!(
                send(&rex, &pane("local"), &expect(), &Action::Pick('1')),
                Err(text.to_owned())
            );
        }
    }

    #[test]
    fn close_asks_rex_to_close_the_block_on_its_host() {
        let dir = tempfile::tempdir().unwrap();
        let rex = fake_rex(dir.path(), &json!({}));
        let argv = || std::fs::read_to_string(dir.path().join("argv")).unwrap();
        close(&rex, &pane("mini")).unwrap();
        assert_eq!(argv(), "-S mini block close block:seat-b\n");
        close(&rex, &pane("local")).unwrap();
        assert_eq!(argv(), "block close block:seat-b\n", "no -S for this host");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("env")).unwrap(),
            "",
            "no REX_BLOCK, REX_SESSION or REX_CLIENT reaches rex"
        );
    }

    #[test]
    fn close_says_why_rex_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let rex = dir.path().join("rex");
        std::fs::write(
            &rex,
            "#!/bin/sh\necho 'error: block not found' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&rex, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            close(&rex, &pane("local")),
            Err("close failed: error: block not found".to_owned())
        );
    }

    #[test]
    fn answer_send_lua_refuses_text_into_a_dialog_before_any_event() {
        let lua = include_str!("lua/send.lua");
        let guard = lua.find("if a.action.text and dialog_up() then return refuse(\"dialog_up\")");
        let first_event = lua.find("rex.call(\"block.event\"");
        assert!(
            guard.is_some() && guard < first_event,
            "the dialog check runs before a paste"
        );
    }

    #[test]
    fn answer_send_lua_never_sends_escape_or_ctrl_c() {
        let lua: String = include_str!("lua/send.lua")
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .map(str::to_lowercase)
            .collect();
        for key in ["escape", "\"esc\"", "ctrl", "\\x03", "\\27", "\\x1b"] {
            assert!(!lua.contains(key), "{key}");
        }
    }
}
