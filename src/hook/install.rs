//! `rexboard hooks install|uninstall|status`: puts `rexboard hook` and `rexboard usage` into each
//! harness's own settings, or takes out exactly those, which the harness rewrites at runtime and
//! which hold private entries, so they are merged, never replaced. Every other key and hook stays,
//! and a second run changes nothing.

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
/// grok reads a command status line only from its own config.toml, which rexboard does not edit.
const GROK_LINE: &str = "[ui.status_line]\ntype = \"command\"\ncommand = \"rexboard usage grok\"";

/// What install would do, or did, to one harness.
#[derive(Debug, Serialize)]
pub struct Plan {
    pub harness: &'static str,
    pub file: Option<String>,
    /// `installed` (nothing to change), `not installed` (nothing to take out), `changes`,
    /// `written`, `removed`, `absent`, `symlink`, `not ours` or `not managed`.
    pub state: &'static str,
    pub changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    #[serde(skip)]
    after: Option<Write>,
}

/// What a run writes to a harness's file.
#[derive(Debug)]
enum Write {
    Text(String),
    /// Install made the file (no backup holds what is left) and nothing else is in it now.
    Remove,
}

/// Which way a run goes: put rexboard's hooks in, or take them out.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Install,
    Uninstall,
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

/// How a run ends: what `status` found, what a dry run of this command would write, or what was
/// written.
#[derive(Clone, Copy, PartialEq)]
enum Ending {
    Status,
    Preview(&'static str),
    Written,
}

/// `rexboard hooks status`, and whether the default tmux server passes a status through.
pub fn status(json: bool) -> Result<()> {
    let plans = HARNESSES
        .iter()
        .map(|h| plan(h, &binary(), Mode::Install))
        .collect::<Result<Vec<_>>>()?;
    let passthrough = super::tmux(&["show", "-gv", "allow-passthrough"]);
    if json {
        println!(
            "{}",
            json!({ "harnesses": plans, "tmux": { "allow_passthrough": passthrough } })
        );
        return Ok(());
    }
    print(&plans, Ending::Status);
    match passthrough.as_deref() {
        Some("on" | "all") => println!("{:<7}allow-passthrough on", "tmux"),
        Some(_) => println!(
            "{:<7}allow-passthrough off: add `set -g allow-passthrough on` to tmux.conf, or a \
             status from inside tmux never reaches Rex",
            "tmux"
        ),
        None => {}
    }
    Ok(())
}

/// `rexboard hooks install|uninstall [HARNESS...]`: every harness this host has, or the ones named.
pub fn run(mode: Mode, names: &[String], dry_run: bool, json: bool) -> Result<()> {
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
            let mut plan = plan(harness, &bin, mode)?;
            if !dry_run && let (Some(after), Some(file)) = (plan.after.take(), &plan.file) {
                let file = expand(file);
                (plan.backup, plan.state) = match after {
                    Write::Text(text) => (apply(&file, &text)?, "written"),
                    Write::Remove => (Some(remove(&file)?), "removed"),
                };
            }
            plans.push(plan);
        }
    }
    let ending = match (dry_run, mode) {
        (false, _) => Ending::Written,
        (true, Mode::Install) => Ending::Preview("install"),
        (true, Mode::Uninstall) => Ending::Preview("uninstall"),
    };
    if json {
        println!("{}", json!({ "harnesses": plans }));
    } else {
        print(&plans, ending);
    }
    Ok(())
}

fn print(plans: &[Plan], ending: Ending) {
    for plan in plans {
        let file = plan.file.as_deref().unwrap_or("-");
        println!("{:<7}{:<14}{file}", plan.harness, plan.state);
        for change in &plan.changes {
            println!("  {change}");
        }
        for line in plan.note.iter().flat_map(|note| note.lines()) {
            println!("  {line}");
        }
        if let Some(backup) = &plan.backup {
            println!("  the old one is {backup}");
        }
    }
    if plans.iter().any(|p| p.state == "changes") {
        match ending {
            Ending::Status => println!("rexboard hooks install writes these"),
            Ending::Preview(command) => {
                println!("dry run: nothing written; rexboard hooks {command} writes these")
            }
            Ending::Written => {}
        }
    }
}

fn plan(harness: &'static str, bin: &str, mode: Mode) -> Result<Plan> {
    let (dir, file) = match harness {
        "claude" => (dir_from("CLAUDE_CONFIG_DIR", ".claude"), "settings.json"),
        "codex" => (dir_from("CODEX_HOME", ".codex"), "hooks.json"),
        "pi" => (
            dir_from("PI_CODING_AGENT_DIR", ".pi/agent"),
            "extensions/rexboard.js",
        ),
        _ => {
            let mut plan = Plan::new(harness, None, "not managed");
            plan.note = Some(format!(
                "grok reports to a Rex pane itself. For its usage, add to ~/.grok/config.toml:\n\
                 {GROK_LINE}"
            ));
            return Ok(plan);
        }
    };
    let file = dir.join(file);
    let found = |file: PathBuf, mode: Mode| match harness {
        "claude" => settings(harness, file, CLAUDE_EVENTS, true, bin, mode),
        "codex" => settings(harness, file, CODEX_EVENTS, false, bin, mode),
        _ => pi(&file, bin, mode),
    };
    let Some(link) = link_on_the_way(&dir, &file) else {
        return found(file, mode);
    };
    // Read through the link, never written: what an install would add is what the file lacks.
    let lacks = (mode == Mode::Install)
        .then(|| found(file.clone(), Mode::Install).ok())
        .flatten()
        .map(|plan| plan.changes);
    let what = match lacks.as_deref() {
        Some([]) => "; it has rexboard's hooks".to_owned(),
        Some(changes) => {
            let names: Vec<&str> = changes
                .iter()
                .map(|change| change.split(':').next().unwrap_or_default())
                .collect();
            format!("; missing: {}", names.join(", "))
        }
        None => String::new(),
    };
    let mut plan = Plan::new(harness, Some(&file), "symlink");
    plan.note = Some(format!(
        "{} is a link into a checkout{what}; change it there",
        shown(&link)
    ));
    Ok(plan)
}

/// The first link a write to `file` would go through: `file` itself, or a directory between it and
/// the harness's home `dir`, from the home directory down when `dir` is under it. Links above that
/// are the system's (macOS's `/var`), not a checkout's.
fn link_on_the_way(dir: &Path, file: &Path) -> Option<PathBuf> {
    let home = home();
    let top = if !home.as_os_str().is_empty() && dir.starts_with(&home) {
        home
    } else {
        dir.parent()?.to_path_buf()
    };
    file.ancestors()
        .take_while(|path| *path != top)
        .find(|path| path.is_symlink())
        .map(Path::to_path_buf)
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

/// How a hook runs rexboard: by full path, since an agent may run its hooks with a shorter PATH
/// than your shell's. That is the `rexboard` on PATH as found (`/opt/homebrew/bin/rexboard`, which
/// outlives an upgrade) when it is this binary, else this binary's own path.
fn binary() -> String {
    let me = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .ok();
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("rexboard"))
            .find(|candidate| candidate.is_absolute() && candidate.is_file())
    });
    let path = match (me, found) {
        (Some(me), Some(found)) if found.canonicalize().ok() == Some(me.clone()) => found,
        (Some(me), _) => me,
        (None, Some(found)) => found,
        (None, None) => return "rexboard".into(),
    };
    quoted(&path.display().to_string())
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

/// A hook install added.
fn ours(hook: &Value) -> bool {
    hook["command"].as_str().and_then(installed_bin).is_some()
}

/// The rexboard a hook install wrote runs, as written, or None for any other hook. After one
/// `sh -c`, its words are exactly `REXBOARD hook HARNESS EVENT`, alone or with `|| true`; a hook
/// of the user's that also runs something else is theirs.
fn installed_bin(command: &str) -> Option<String> {
    let mut words = shell_words(command);
    if let [shell, flag, script] = &words[..]
        && Path::new(shell).file_name().is_some_and(|n| n == "sh")
        && flag == "-c"
    {
        words = shell_words(script);
    }
    let whole = match &words[..] {
        [_, _, _, _] => true,
        [_, _, _, _, or, yes] => or == "||" && yes == "true",
        _ => false,
    };
    let [bin, verb, ..] = &words[..] else {
        return None;
    };
    let named = Path::new(bin).file_name().is_some_and(|n| n == "rexboard");
    (whole && named && verb == "hook").then(|| bin.clone())
}

/// A hook that reports to Rex: rexboard's own, or the `rex-agent-status` bridge it replaces.
fn reports(hook: &Value) -> bool {
    let command = hook["command"].as_str().unwrap_or_default();
    command.contains("rex-agent-status") || runs_hook(command, 2)
}

/// Whether `command`, read as a shell reads it, runs `rexboard hook`: its program is a file named
/// `rexboard` and its first argument `hook`, also inside `sh -c '…'`, however the path is quoted.
fn runs_hook(command: &str, depth: u8) -> bool {
    let words = shell_words(command);
    let named = |word: &str| {
        Path::new(word)
            .file_name()
            .is_some_and(|name| name == "rexboard")
    };
    if depth > 0
        && let [shell, flag, script, ..] = &words[..]
        && Path::new(shell)
            .file_name()
            .is_some_and(|name| name == "sh")
        && flag == "-c"
    {
        return runs_hook(script, depth - 1);
    }
    matches!(&words[..], [program, verb, ..] if named(program) && verb == "hook")
}

/// `command` split into words as a POSIX shell splits them: whitespace between, `'…'` literal,
/// `"…"` with `\` before `"`, `\`, `$` and `` ` ``, and `\` quoting the next character.
fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => words.extend(word.take()),
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                w.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(next) if "\"\\$`".contains(next) => w.push(next),
                            Some(next) => {
                                w.push('\\');
                                w.push(next);
                            }
                            None => w.push('\\'),
                        },
                        c => w.push(c),
                    }
                }
            }
            '\\' => word.get_or_insert_with(String::new).extend(chars.next()),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    words
}

/// A hook that exits 0 whatever happens, since exit 2 blocks the agent: a missing rexboard, or one
/// too old to know `hook`, fails open.
fn hook_command(bin: &str, harness: &str, event: &str) -> String {
    format!(
        "/bin/sh -c {}",
        quoted(&format!("{bin} hook {harness} {event} || true"))
    )
}

fn settings(
    harness: &'static str,
    file: PathBuf,
    events: &[Event],
    status_line: bool,
    bin: &str,
    mode: Mode,
) -> Result<Plan> {
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(_) if mode == Mode::Uninstall && !file.exists() => {
            return Ok(Plan::new(harness, Some(&file), "not installed"));
        }
        Err(_) if file.parent().is_some_and(Path::is_dir) && !file.exists() => "{}\n".into(),
        Err(_) if !file.exists() => return Ok(Plan::new(harness, Some(&file), "absent")),
        Err(err) => return Err(Error::Config(format!("{}: {err}", shown(&file)))),
    };
    let live: Value = serde_json::from_str(&before)
        .map_err(|err| Error::Config(format!("{} is not valid JSON: {err}", shown(&file))))?;
    let known = |groups: &Value| {
        groups.as_array().is_some_and(|groups| {
            groups
                .iter()
                .all(|group| group.is_object() && group["hooks"].is_array())
        })
    };
    let hooks_known = live["hooks"].is_null()
        || live["hooks"]
            .as_object()
            .is_some_and(|hooks| hooks.values().all(known));
    if !live.is_object() || !hooks_known {
        return Err(Error::Config(format!(
            "{} has hooks in a shape rexboard does not know; left alone",
            shown(&file)
        )));
    }
    let found = (mode == Mode::Uninstall)
        .then(|| before_install(&file, &live, harness, events, status_line, bin))
        .flatten();
    let (mut after, done) = match (mode, &found) {
        (Mode::Install, _) => (merged(&live, harness, events, bin), "installed"),
        (Mode::Uninstall, Some((_, was))) => (was.clone(), "not installed"),
        (Mode::Uninstall, None) => (stripped(&live), "not installed"),
    };
    let mut changes = changes(&live, &after, events);
    let line = &live["statusLine"];
    if status_line && mode == Mode::Install {
        if let Some(new) = usage_line(line, bin) {
            changes.push(format!(
                "statusLine: runs {} through rexboard usage claude",
                line["command"]
                    .as_str()
                    .map_or("nothing".to_owned(), |c| format!("`{c}`"))
            ));
            after["statusLine"] = new;
        }
    } else if status_line {
        let restored = found.is_some();
        match unchained(line) {
            Some(Some(command)) => {
                changes.push(format!("statusLine: back to `{command}` alone"));
                if !restored {
                    after["statusLine"]["command"] = json!(command);
                }
            }
            Some(None) => {
                changes.push("statusLine: remove `rexboard usage claude`".into());
                if !restored {
                    let object = after.as_object_mut().expect("an object");
                    object.shift_remove("statusLine");
                }
            }
            None => {}
        }
    }
    let mut plan = Plan::new(
        harness,
        Some(&file),
        if changes.is_empty() { done } else { "changes" },
    );
    if !changes.is_empty() {
        plan.after = Some(if let Some((text, _)) = found {
            Write::Text(text)
        } else if after.as_object().is_some_and(|o| o.is_empty()) {
            Write::Remove
        } else {
            let end = if before.ends_with('\n') { "\n" } else { "" };
            let text = serde_json::to_string_pretty(&after).map_err(std::io::Error::other)?;
            Write::Text(text + end)
        });
    }
    plan.changes = changes;
    Ok(plan)
}

/// The file as install found it, text and value: the newest backup that holds nothing of
/// rexboard's and that install turns into `live`, run with the rexboard `live`'s hooks name (or
/// `bin`). A backup holding rexboard's hooks is a later install's, and is passed over. None when
/// the file changed since install, or install made it.
fn before_install(
    file: &Path,
    live: &Value,
    harness: &str,
    events: &[Event],
    status_line: bool,
    bin: &str,
) -> Option<(String, Value)> {
    let mut bins = vec![bin.to_owned()];
    for hook in live["hooks"]
        .as_object()
        .into_iter()
        .flat_map(|hooks| hooks.values())
        .flat_map(|groups| groups.as_array().into_iter().flatten())
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
    {
        if let Some(found) = hook["command"].as_str().and_then(installed_bin) {
            bins.push(quoted(&found));
        }
    }
    bins.dedup();
    let name = format!("{}.bak-", file.file_name()?.to_string_lossy());
    let mut backups: Vec<PathBuf> = std::fs::read_dir(file.parent()?)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&name))
        })
        .collect();
    backups.sort();
    backups.into_iter().rev().find_map(|path| {
        let text = std::fs::read_to_string(path).ok()?;
        let was: Value = serde_json::from_str(&text).ok()?;
        let clean =
            was.is_object() && stripped(&was) == was && unchained(&was["statusLine"]).is_none();
        let becomes = |bin: &String| {
            let mut out = merged(&was, harness, events, bin);
            if status_line && let Some(line) = usage_line(&was["statusLine"], bin) {
                out["statusLine"] = line;
            }
            out == *live
        };
        (clean && bins.iter().any(becomes)).then_some((text, was))
    })
}

/// `live` without the hooks install added. A group, an event or `hooks` that only held them goes;
/// one the user left empty stays.
fn stripped(live: &Value) -> Value {
    let mut out = live.clone();
    // Not `out["hooks"]`: indexing a map for a key it lacks inserts it.
    let Some(hooks) = out.get_mut("hooks").and_then(Value::as_object_mut) else {
        return out;
    };
    let mut emptied = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let had = groups.len();
        groups.retain_mut(|group| {
            let Some(list) = group["hooks"].as_array_mut() else {
                return true;
            };
            let before = list.len();
            list.retain(|hook| !ours(hook));
            list.len() == before || !list.is_empty()
        });
        if groups.is_empty() && had > 0 {
            emptied.push(event.clone());
        }
    }
    for event in emptied {
        hooks.shift_remove(&event);
    }
    if hooks.is_empty() && !live["hooks"].as_object().is_some_and(|h| h.is_empty()) {
        out.as_object_mut()
            .expect("an object")
            .shift_remove("hooks");
    }
    out
}

/// The status line as it was before install: Some(Some(the user's own command)) for one install
/// chained, Some(None) for one install added, None for one install did not write.
fn unchained(current: &Value) -> Option<Option<String>> {
    let command = current["command"].as_str()?;
    let words = shell_words(command);
    let program = words.first()?;
    if Path::new(program).file_name()? != "rexboard" {
        return None;
    }
    let bin = quoted(program);
    if command == format!("{bin} usage claude") {
        return Some(None);
    }
    let theirs = words.last()?;
    (usage_line(&json!({ "command": theirs }), &bin)?["command"] == command)
        .then(|| Some(theirs.clone()))
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
            hooks.shift_remove(&event);
        } else {
            hooks.insert(event, Value::Array(new));
        }
    }
    out
}

/// Each event whose reporting hooks change: `EVENT: add …, keeping your 1 hook`,
/// `replace … with …`, or `remove …`.
fn changes(live: &Value, after: &Value, events: &[Event]) -> Vec<String> {
    let hooks = |settings: &Value, event: &str| -> Vec<Value> {
        settings["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["hooks"].as_array().cloned().unwrap_or_default())
            .collect()
    };
    let reporting = |settings: &Value, event: &str| -> Vec<String> {
        hooks(settings, event)
            .iter()
            .filter(|hook| reports(hook))
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
            if old == new {
                return None;
            }
            let yours = hooks(after, event).iter().filter(|h| !reports(h)).count();
            Some(match (old.is_empty(), new.is_empty()) {
                (true, _) if yours > 0 => format!(
                    "{event}: add {}, keeping your {yours} hook{}",
                    said(&new),
                    if yours == 1 { "" } else { "s" }
                ),
                (true, _) => format!("{event}: add {}", said(&new)),
                (_, true) => format!("{event}: remove {}", said(&old)),
                _ => format!("{event}: replace {} with {}", said(&old), said(&new)),
            })
        })
        .collect()
}

/// Commands as a change shows them, each in backticks.
fn said(commands: &[String]) -> String {
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
    // A missing rexboard, or one too old to know `usage`, leaves the user's own line running.
    let found = format!("{bin} usage --help >/dev/null 2>&1");
    let theirs = quoted(command);
    let mut line = current.clone();
    line["command"] = json!(format!(
        "{found} && exec {mine} -- /bin/sh -c {theirs} || exec /bin/sh -c {theirs}"
    ));
    Some(line)
}

fn pi(file: &Path, bin: &str, mode: Mode) -> Result<Plan> {
    let dir = file.parent().unwrap_or(Path::new("."));
    if mode == Mode::Uninstall {
        let have = std::fs::read_to_string(file).ok();
        let (before, after) = PI_EXTENSION
            .split_once("__REXBOARD__")
            .expect("a placeholder");
        let rexboards = have.as_deref().is_some_and(|text| {
            text.strip_prefix(before)
                .and_then(|rest| rest.strip_suffix(after))
                .is_some_and(|bin| serde_json::from_str::<String>(bin).is_ok())
        });
        let mut plan = Plan::new("pi", Some(file), "not installed");
        if rexboards {
            plan.state = "changes";
            plan.changes.push("the extension: remove it".into());
            plan.after = Some(Write::Remove);
        } else if have.is_some() {
            plan.state = "not ours";
            plan.note = Some("not the extension rexboard wrote; left alone".into());
        }
        return Ok(plan);
    }
    if !dir.parent().is_some_and(Path::is_dir) {
        return Ok(Plan::new("pi", Some(file), "absent"));
    }
    let want = PI_EXTENSION.replace("__REXBOARD__", &json!(bin).to_string());
    let have = std::fs::read_to_string(file).ok();
    let mut plan = Plan::new("pi", Some(file), "installed");
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
        plan.after = Some(Write::Text(want));
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
            let backup = keep_old(file, mode)?;
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

/// Removes `file` after a backup beside it: the backup's path.
fn remove(file: &Path) -> Result<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(file)?.permissions().mode() & 0o7777;
    let backup = keep_old(file, mode)?;
    std::fs::remove_file(file)?;
    Ok(shown(&backup))
}

/// Copies `file` to `FILE.bak-<UTC>` with `mode`, never over an existing backup: a second one in
/// the same second takes `-2`, then `-3`.
fn keep_old(file: &Path, mode: u32) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let base = format!(
        "{}.bak-{}",
        file.file_name().unwrap_or_default().to_string_lossy(),
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let old = std::fs::read(file)?;
    for n in 1.. {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let backup = file.with_file_name(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&backup)
        {
            Ok(mut out) => {
                out.write_all(&old)?;
                std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(mode))?;
                return Ok(backup);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    unreachable!("some name is free")
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
                { "hooks": [{ "type": "command", "command": "/bin/sh -c 'rexboard hook claude Stop || true'", "timeout": 5 }] },
                { "hooks": [{ "type": "command", "command": "notify" }] },
            ]),
            "in the bridge's place; the user's hooks stay"
        );
        assert_eq!(after["hooks"]["PreCompact"], live["hooks"]["PreCompact"]);
        assert_eq!(
            after["hooks"]["PostToolUse"],
            json!([{ "matcher": "AskUserQuestion", "hooks": [
                { "type": "command", "command": "/bin/sh -c 'rexboard hook claude PostToolUse || true'", "timeout": 5 }
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
            said.contains(
                &"Stop: replace `rex-agent-status` with `/bin/sh -c 'rexboard hook claude Stop || true'`".to_owned()
            ) && said.contains(&"PreToolUse: add `/bin/sh -c 'rexboard hook claude PreToolUse || true'`".to_owned()),
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

    #[test]
    fn an_emptied_event_goes_and_the_rest_keep_their_order() {
        let live = json!({ "hooks": {
            "Gone": [{ "hooks": [bridge("Gone")] }],
            "PreCompact": [{ "hooks": [{ "type": "command", "command": "mine" }] }],
            "Custom": [{ "hooks": [{ "type": "command", "command": "also mine" }] }],
        }});
        let after = merged(&live, "codex", CODEX_EVENTS, "rexboard");
        let keys: Vec<&String> = after["hooks"].as_object().unwrap().keys().collect();
        assert_eq!(keys[..2], ["PreCompact", "Custom"], "{keys:?}");
        assert!(
            changes(&live, &after, CODEX_EVENTS)
                .contains(&"Gone: remove `rex-agent-status`".to_owned())
        );
    }

    #[test]
    fn a_settings_file_with_hooks_in_another_shape_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        for text in [
            r#"{"hooks": {"Stop": {"hooks": []}}}"#,
            r#"{"hooks": {"Stop": [{"hooks": {"type": "command"}}]}}"#,
            r#"{"hooks": {"Stop": ["rexboard hook claude Stop"]}}"#,
        ] {
            std::fs::write(&file, text).unwrap();
            let err = settings(
                "claude",
                file.clone(),
                CLAUDE_EVENTS,
                true,
                "rexboard",
                Mode::Install,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("a shape rexboard does not know"),
                "{text}: {err}"
            );
        }
    }

    #[test]
    fn two_writes_in_one_second_keep_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hooks.json");
        std::fs::write(&file, "original").unwrap();
        let first = apply(&file, "first").unwrap().unwrap();
        let second = apply(&file, "second").unwrap().unwrap();
        assert_ne!(first, second);
        let mut kept: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, ["first", "original", "second"]);
    }

    #[test]
    fn a_hook_is_rexboards_however_its_path_is_quoted() {
        for bin in [
            "/opt/homebrew/bin/rexboard",
            "/tmp/My Tools/rexboard",
            "/tmp/it's/rexboard",
            "/tmp/a \"b\" $c/rexboard",
        ] {
            let command = hook_command(&quoted(bin), "claude", "Stop");
            assert!(reports(&json!({ "command": command })), "{command}");
            let inner = &shell_words(&command)[2];
            assert_eq!(shell_words(inner)[0], bin, "{command}");
        }
        for theirs in [
            "afplay done.aiff",
            "/usr/local/bin/rexboard-notes hook",
            "/bin/sh -c 'echo rexboard hook'",
            "echo 'rexboard'",
        ] {
            assert!(!reports(&json!({ "command": theirs })), "{theirs}");
        }
        assert!(
            reports(&json!({ "command": "rexboard hook claude Stop" })),
            "the bare form"
        );
        assert_eq!(
            shell_words(r#"a 'b c' "d \"e\" \$f" g\ h"#),
            ["a", "b c", r#"d "e" $f"#, "g h"]
        );
    }

    #[test]
    fn uninstall_unwinds_only_the_status_line_install_wrote() {
        for bin in ["/opt/homebrew/bin/rexboard", "'/tmp/it'\\''s/rexboard'"] {
            let theirs = "~/bin/my line.sh --short 'a b'";
            let chained = usage_line(&json!({ "command": theirs }), bin).unwrap();
            assert_eq!(unchained(&chained), Some(Some(theirs.to_owned())), "{bin}");
            let added = usage_line(&Value::Null, bin).unwrap();
            assert_eq!(unchained(&added), Some(None), "{bin}");
        }
        for not_ours in [
            "~/bin/line.sh",
            "rexboard usage claude -- my-line.sh",
            "echo usage claude",
        ] {
            assert_eq!(
                unchained(&json!({ "command": not_ours })),
                None,
                "{not_ours}"
            );
        }
    }

    #[test]
    fn uninstall_takes_out_only_rexboards_hooks_and_what_they_alone_filled() {
        let live = json!({
            "hooks": {
                "Mine": [],
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "say done" }] },
                    { "hooks": [{ "type": "command", "command": hook_command("rexboard", "claude", "Stop") }] }
                ],
                "SessionEnd": [
                    { "hooks": [{ "type": "command", "command": hook_command("rexboard", "claude", "SessionEnd") }] }
                ]
            }
        });
        let out = stripped(&live);
        assert_eq!(
            out,
            json!({ "hooks": {
                "Mine": [],
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
            }}),
            "the user's own empty event stays"
        );
        let only =
            json!({ "model": "opus", "hooks": { "SessionEnd": live["hooks"]["SessionEnd"] } });
        assert_eq!(
            stripped(&only),
            json!({ "model": "opus" }),
            "hooks only held rexboard's"
        );
    }

    #[test]
    fn only_the_hooks_install_writes_are_rexboards_to_take_out() {
        for (command, bin) in [
            (
                hook_command("/opt/homebrew/bin/rexboard", "claude", "Stop"),
                Some("/opt/homebrew/bin/rexboard"),
            ),
            (
                hook_command("'/tmp/My Tools/rexboard'", "codex", "Stop"),
                Some("/tmp/My Tools/rexboard"),
            ),
            ("rexboard hook claude Stop".into(), Some("rexboard")),
            (
                "/bin/sh -c 'rexboard hook claude Notification; afplay ping.aiff'".into(),
                None,
            ),
            (
                "rexboard hook claude PreCompact && notify-send compacting".into(),
                None,
            ),
            ("rexboard hook claude Stop || say done".into(), None),
            ("rexboard usage claude".into(), None),
            ("echo rexboard hook claude Stop".into(), None),
        ] {
            assert_eq!(installed_bin(&command).as_deref(), bin, "{command}");
        }
    }
}
