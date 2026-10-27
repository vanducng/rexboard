//! `rexboard hooks install|status`: puts `rexboard hook` and `rexboard usage` into each harness's
//! own settings, which the harness rewrites at runtime and which hold private entries, so they
//! are merged, never replaced. Every other key and hook stays, and a second install changes
//! nothing.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};

/// A hook event, with the matcher its group needs.
type Event = (&'static str, Option<&'static str>);

const CLAUDE_EVENTS: &[Event] = &[
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", None),
    ("PostToolUse", Some("AskUserQuestion")),
    ("PermissionRequest", None),
    ("Notification", None),
    ("Stop", None),
    ("StopFailure", None),
    ("SessionEnd", None),
];
const CODEX_EVENTS: &[Event] = &[
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", None),
    ("PermissionRequest", None),
    ("Stop", None),
    ("SessionEnd", None),
];
/// The pi extension, with `__REXBOARD__` for the command that runs rexboard.
const PI_EXTENSION: &str = include_str!("pi-extension.js");
const HARNESSES: [&str; 4] = ["claude", "codex", "pi", "grok"];

/// What install would do, or did, to one harness.
#[derive(Debug, Serialize)]
pub struct Plan {
    pub harness: &'static str,
    pub file: Option<String>,
    /// `installed` (nothing to change), `changes`, `written`, `absent`, `symlink` or
    /// `not managed`.
    pub state: &'static str,
    pub changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    #[serde(skip)]
    after: Option<String>,
}

impl Plan {
    fn new(harness: &'static str, file: Option<&Path>, state: &'static str) -> Plan {
        Plan {
            harness,
            file: file.map(shown),
            state,
            changes: Vec::new(),
            note: None,
            backup: None,
            after: None,
        }
    }
}

/// `rexboard hooks status`.
pub fn status(json: bool) -> Result<()> {
    let plans = HARNESSES
        .iter()
        .map(|h| plan(h, &binary()))
        .collect::<Result<Vec<_>>>()?;
    print(&plans, json, false);
    Ok(())
}

/// `rexboard hooks install [HARNESS...]`: every harness this host has, or the ones named.
pub fn install(names: &[String], dry_run: bool, json: bool) -> Result<()> {
    for name in names {
        if !HARNESSES.contains(&name.as_str()) {
            return Err(Error::usage_with(
                format!(
                    "no harness named {name}; harnesses: {}",
                    HARNESSES.join(", ")
                ),
                &["rexboard hooks status"],
            ));
        }
    }
    let bin = binary();
    let mut plans = Vec::new();
    for harness in HARNESSES {
        if names.is_empty() || names.iter().any(|n| n == harness) {
            let mut plan = plan(harness, &bin)?;
            if !dry_run && let (Some(after), Some(file)) = (plan.after.take(), &plan.file) {
                plan.backup = apply(&expand(file), &after)?;
                plan.state = "written";
            }
            plans.push(plan);
        }
    }
    print(&plans, json, !dry_run);
    Ok(())
}

fn print(plans: &[Plan], json: bool, applied: bool) {
    if json {
        println!("{}", json!({ "harnesses": plans }));
        return;
    }
    for plan in plans {
        let file = plan.file.as_deref().unwrap_or("-");
        println!("{}  {}  {}", plan.harness, plan.state, file);
        for change in &plan.changes {
            println!("  {change}");
        }
        if let Some(note) = &plan.note {
            println!("  {note}");
        }
        match (&plan.backup, plan.state) {
            (Some(backup), _) => println!("  the old one is {backup}"),
            (None, "changes") if !applied => println!("  rexboard hooks install writes it"),
            _ => {}
        }
    }
}

fn plan(harness: &'static str, bin: &str) -> Result<Plan> {
    match harness {
        "claude" => settings(harness, claude_file(), CLAUDE_EVENTS, true, bin),
        "codex" => settings(harness, codex_file(), CODEX_EVENTS, false, bin),
        "pi" => pi(bin),
        _ => {
            let mut plan = Plan::new(harness, None, "not managed");
            plan.note = Some(
                "grok reports to a Rex pane itself; its status line is read only from \
                 ~/.grok/config.toml, so add `rexboard usage grok` there by hand"
                    .into(),
            );
            Ok(plan)
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn dir_from(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(default))
}

fn claude_file() -> PathBuf {
    dir_from("CLAUDE_CONFIG_DIR", ".claude").join("settings.json")
}

fn codex_file() -> PathBuf {
    dir_from("CODEX_HOME", ".codex").join("hooks.json")
}

fn pi_dir() -> PathBuf {
    dir_from("PI_CODING_AGENT_DIR", ".pi/agent").join("extensions")
}

/// `~/` for the home directory, so a printed path names no user.
fn shown(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) if !home().as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

fn expand(file: &str) -> PathBuf {
    match file.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(file),
    }
}

/// How a hook runs rexboard: `rexboard` when that is this binary on PATH, else its full path.
fn binary() -> String {
    let me = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .ok();
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("rexboard"))
            .find(|candidate| candidate.is_file())
            .and_then(|found| found.canonicalize().ok())
    });
    match me {
        Some(me) if on_path.as_ref() != Some(&me) => quoted(&me.display().to_string()),
        _ => "rexboard".into(),
    }
}

/// `text` as one shell word.
fn quoted(text: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+".contains(c);
    if !text.is_empty() && text.chars().all(plain) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// A hook that reports to Rex: rexboard's own, or the `rex-agent-status` bridge it replaces.
fn reports(hook: &Value) -> bool {
    let command = hook["command"].as_str().unwrap_or_default();
    let mut words = command.split_whitespace();
    command.contains("rex-agent-status")
        || (words
            .next()
            .is_some_and(|w| w.trim_matches('\'').ends_with("rexboard"))
            && words.next() == Some("hook"))
}

fn hook_command(bin: &str, harness: &str, event: &str) -> String {
    format!("{bin} hook {harness} {event}")
}

fn settings(
    harness: &'static str,
    file: PathBuf,
    events: &[Event],
    status_line: bool,
    bin: &str,
) -> Result<Plan> {
    if file.is_symlink() {
        let mut plan = Plan::new(harness, Some(&file), "symlink");
        plan.note = Some("a link, as a dotfiles checkout makes it: change it there".into());
        return Ok(plan);
    }
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(_) if file.parent().is_some_and(Path::is_dir) && !file.exists() => "{}\n".into(),
        Err(_) if !file.exists() => return Ok(Plan::new(harness, Some(&file), "absent")),
        Err(err) => return Err(Error::Config(format!("{}: {err}", shown(&file)))),
    };
    let live: Value = serde_json::from_str(&before)
        .map_err(|err| Error::Config(format!("{} is not valid JSON: {err}", shown(&file))))?;
    if !live.is_object() || !(live["hooks"].is_null() || live["hooks"].is_object()) {
        return Err(Error::Config(format!(
            "{} has hooks in a shape rexboard does not know; left alone",
            shown(&file)
        )));
    }
    let mut after = merged(&live, harness, events, bin);
    let mut changes = changes(&live, &after, events);
    if status_line && let Some(line) = usage_line(&live["statusLine"], bin) {
        changes.push(format!(
            "statusLine: runs {} through rexboard usage claude",
            live["statusLine"]["command"]
                .as_str()
                .map_or("nothing".to_owned(), |c| format!("`{c}`"))
        ));
        after["statusLine"] = line;
    }
    let mut plan = Plan::new(
        harness,
        Some(&file),
        if changes.is_empty() {
            "installed"
        } else {
            "changes"
        },
    );
    if !changes.is_empty() {
        let end = if before.ends_with('\n') { "\n" } else { "" };
        let text = serde_json::to_string_pretty(&after).map_err(std::io::Error::other)?;
        plan.after = Some(text + end);
    }
    plan.changes = changes;
    Ok(plan)
}

/// `live` with its reporting hooks replaced by rexboard's, each event's where the old ones were.
fn merged(live: &Value, harness: &str, events: &[Event], bin: &str) -> Value {
    let mut out = live.clone();
    if !out["hooks"].is_object() {
        out["hooks"] = json!({});
    }
    let hooks = out["hooks"].as_object_mut().expect("an object");
    let want = |event: &str| -> Vec<Value> {
        events
            .iter()
            .filter(|(name, _)| *name == event)
            .map(|(name, matcher)| {
                let mut group = json!({});
                if let Some(matcher) = matcher {
                    group["matcher"] = json!(matcher);
                }
                group["hooks"] = json!([{
                    "type": "command",
                    "command": hook_command(bin, harness, name),
                    "timeout": 5,
                }]);
                group
            })
            .collect()
    };
    let mut names: Vec<String> = hooks.keys().cloned().collect();
    names.extend(
        events
            .iter()
            .map(|(name, _)| name.to_string())
            .filter(|name| !hooks.contains_key(name)),
    );
    for event in names {
        let mut new = Vec::new();
        let mut placed = false;
        for group in hooks
            .get(&event)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let all = group["hooks"].as_array().cloned().unwrap_or_default();
            let rest: Vec<Value> = all.iter().filter(|h| !reports(h)).cloned().collect();
            if rest.len() < all.len() && !placed {
                new.extend(want(&event));
                placed = true;
            }
            if !rest.is_empty() {
                let mut kept = group.clone();
                kept["hooks"] = Value::Array(rest);
                new.push(kept);
            }
        }
        if !placed {
            new.extend(want(&event));
        }
        if new.is_empty() {
            hooks.remove(&event);
        } else {
            hooks.insert(event, Value::Array(new));
        }
    }
    out
}

/// Each event whose reporting hooks change, as `EVENT: old -> new`.
fn changes(live: &Value, after: &Value, events: &[Event]) -> Vec<String> {
    let reporting = |settings: &Value, event: &str| -> Vec<String> {
        settings["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["hooks"].as_array().cloned().unwrap_or_default())
            .filter(reports)
            .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
            .collect()
    };
    let mut names: Vec<&str> = events.iter().map(|(name, _)| *name).collect();
    names.extend(
        live["hooks"]
            .as_object()
            .into_iter()
            .flat_map(|hooks| hooks.keys())
            .map(String::as_str)
            .filter(|name| !events.iter().any(|(e, _)| e == name)),
    );
    names
        .into_iter()
        .filter_map(|event| {
            let (old, new) = (reporting(live, event), reporting(after, event));
            (old != new).then(|| format!("{event}: {} -> {}", said(&old), said(&new)))
        })
        .collect()
}

/// Commands as a change shows them: each in backticks, or `none`.
fn said(commands: &[String]) -> String {
    if commands.is_empty() {
        return "none".to_owned();
    }
    commands
        .iter()
        .map(|c| format!("`{}`", short(c)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A long bridge command by the script it runs.
fn short(command: &str) -> &str {
    if command.contains("rex-agent-status") {
        "rex-agent-status"
    } else {
        command
    }
}

/// The status line that runs the user's own through `rexboard usage claude`, or None when it
/// already does or is not a command. Without rexboard the user's own still runs.
fn usage_line(current: &Value, bin: &str) -> Option<Value> {
    let mine = format!("{bin} usage claude");
    let Some(command) = current["command"].as_str() else {
        return current
            .is_null()
            .then(|| json!({ "type": "command", "command": mine }));
    };
    if command.contains("usage claude") {
        return None;
    }
    let found = if bin == "rexboard" {
        "command -v rexboard >/dev/null 2>&1".to_owned()
    } else {
        format!("[ -x {bin} ]")
    };
    let theirs = quoted(command);
    let mut line = current.clone();
    line["command"] = json!(format!(
        "{found} && exec {mine} -- /bin/sh -c {theirs} || exec /bin/sh -c {theirs}"
    ));
    Some(line)
}

fn pi(bin: &str) -> Result<Plan> {
    let dir = pi_dir();
    let file = dir.join("rexboard.js");
    if !dir.parent().is_some_and(Path::is_dir) {
        return Ok(Plan::new("pi", Some(&file), "absent"));
    }
    let want = PI_EXTENSION.replace("__REXBOARD__", &json!(bin).to_string());
    let have = std::fs::read_to_string(&file).ok();
    let mut plan = Plan::new("pi", Some(&file), "installed");
    if have.as_deref() != Some(want.as_str()) {
        plan.state = "changes";
        plan.changes.push(
            if have.is_some() {
                "the extension: replaced with this build's"
            } else {
                "the extension: reports pi's status and keeps each turn's usage"
            }
            .into(),
        );
        plan.after = Some(want);
    }
    if dir.join("rex-status.js").exists() {
        plan.note = Some(format!(
            "{} reports too; remove it, or pi reports twice",
            shown(&dir.join("rex-status.js"))
        ));
    }
    Ok(plan)
}

/// Writes `text` to `file` through a rename, after a backup beside it with the file's own mode,
/// since it holds the same private entries: the backup's path, or None for a new file.
fn apply(file: &Path, text: &str) -> Result<Option<String>> {
    use std::os::unix::fs::PermissionsExt;
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mode = std::fs::metadata(file).map(|m| m.permissions().mode() & 0o7777);
    let put = |path: &Path, body: &str, mode: u32| -> std::io::Result<()> {
        let tmp = dir.join(format!(".rexboard-hooks-{}", std::process::id()));
        std::fs::write(&tmp, body)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        std::fs::rename(&tmp, path)
    };
    let backup = match mode {
        Ok(mode) => {
            let name = format!(
                "{}.bak-{}",
                file.file_name().unwrap_or_default().to_string_lossy(),
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            );
            let backup = dir.join(name);
            put(&backup, &std::fs::read_to_string(file)?, mode)?;
            put(file, text, mode)?;
            Some(shown(&backup))
        }
        Err(_) => {
            put(file, text, 0o644)?;
            None
        }
    };
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge(event: &str) -> Value {
        json!({ "type": "command", "timeout": 5, "command": format!(
            "/bin/sh -c 'exec /usr/bin/python3 \"$HOME/.local/bin/rex-agent-status\" claude-code' # {event}"
        )})
    }

    #[test]
    fn a_merge_puts_rexboards_hooks_where_the_bridge_was_and_keeps_everything_else() {
        let live = json!({
            "model": "opus",
            "hooks": {
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "afplay done.aiff" }] },
                    { "hooks": [bridge("Stop"), { "type": "command", "command": "notify" }] },
                ],
                "PreCompact": [{ "hooks": [{ "type": "command", "command": "mine" }] }],
                "SessionEnd": [{ "hooks": [bridge("SessionEnd")] }],
            },
            "permissions": { "allow": [] },
        });
        let after = merged(&live, "claude", CLAUDE_EVENTS, "rexboard");
        let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "permissions"], "key order kept");
        assert_eq!(
            after["hooks"]["Stop"],
            json!([
                { "hooks": [{ "type": "command", "command": "afplay done.aiff" }] },
                { "hooks": [{ "type": "command", "command": "rexboard hook claude Stop", "timeout": 5 }] },
                { "hooks": [{ "type": "command", "command": "notify" }] },
            ]),
            "in the bridge's place; the user's hooks stay"
        );
        assert_eq!(after["hooks"]["PreCompact"], live["hooks"]["PreCompact"]);
        assert_eq!(
            after["hooks"]["PostToolUse"],
            json!([{ "matcher": "AskUserQuestion", "hooks": [
                { "type": "command", "command": "rexboard hook claude PostToolUse", "timeout": 5 }
            ]}]),
            "a missing event is added"
        );
        assert_eq!(
            merged(&after, "claude", CLAUDE_EVENTS, "rexboard"),
            after,
            "a second merge changes nothing"
        );
        let said = changes(&live, &after, CLAUDE_EVENTS);
        assert!(
            said.contains(&"Stop: `rex-agent-status` -> `rexboard hook claude Stop`".to_owned()),
            "{said:#?}"
        );
        assert!(changes(&after, &after, CLAUDE_EVENTS).is_empty());
    }

    #[test]
    fn the_status_line_runs_through_rexboard_and_without_it_still_runs() {
        let theirs =
            json!({ "type": "command", "command": "\"$HOME/bin/line\" --x 'y'", "padding": 0 });
        let line = usage_line(&theirs, "rexboard").unwrap();
        assert_eq!(line["padding"], 0, "its other keys stay");
        let command = line["command"].as_str().unwrap();
        for (path, exists) in [("/usr/bin:/bin", false), ("PATHWITHREXBOARD", true)] {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("rexboard");
            if exists {
                std::fs::write(&bin, "#!/bin/sh\necho via-rexboard \"$@\"\n").unwrap();
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let path = if exists {
                format!("{}:/usr/bin:/bin", dir.path().display())
            } else {
                path.to_owned()
            };
            let out = std::process::Command::new("/bin/sh")
                .args(["-c", command])
                .env("PATH", path)
                .env("HOME", "/nowhere")
                .output()
                .unwrap();
            let out = String::from_utf8_lossy(&out.stdout).into_owned()
                + &String::from_utf8_lossy(&out.stderr);
            if exists {
                assert!(
                    out.starts_with("via-rexboard usage claude -- /bin/sh -c"),
                    "{out}"
                );
            } else {
                assert!(
                    out.contains("/nowhere/bin/line"),
                    "theirs runs alone: {out}"
                );
            }
        }
        assert_eq!(
            usage_line(&line, "rexboard"),
            None,
            "a second install leaves it"
        );
        assert_eq!(
            usage_line(&Value::Null, "rexboard"),
            Some(json!({ "type": "command", "command": "rexboard usage claude" }))
        );
    }

    #[test]
    fn a_path_with_a_space_is_one_shell_word() {
        assert_eq!(
            quoted("/opt/homebrew/bin/rexboard"),
            "/opt/homebrew/bin/rexboard"
        );
        assert_eq!(
            quoted("/Apps/My Tools/rexboard"),
            "'/Apps/My Tools/rexboard'"
        );
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
