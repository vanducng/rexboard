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
    println!("{}", icons_line());
    Ok(())
}

/// Whether the board draws Nerd Font marks, or how to check the font and turn them on. The board
/// cannot tell the font, so it never turns them on by itself.
fn icons_line() -> String {
    if crate::tui::icons_on(&crate::tui::prefs()) {
        return format!("{:<7}nerd marks on", "icons");
    }
    format!(
        "{:<7}off (words). Nerd Font? This should show one glyph between the bars:\n\
         {:<7}printf '|\\356\\257\\222|\\n'   then set \"icons\": \"nerd\" in tui.json",
        "icons", ""
    )
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

/// How a checkout shared by several hosts finds rexboard: where install.sh puts it, else where
/// Homebrew does. Install writes the path it is running from instead, which is right for this
/// host whatever the install method; this is for a hand-kept file, and is recognised as rexboard's.
const PORTABLE: &str =
    r#"B=$HOME/.local/bin/rexboard; [ -x "$B" ] || B=/opt/homebrew/bin/rexboard"#;
/// The two spellings of a `$HOME` path to rexboard that are recognised, as written.
const HOME_BINS: [&str; 2] = ["$HOME/.local/bin/rexboard", "\"$HOME/.local/bin/rexboard\""];

/// The word `quoted` wrote at the start of `text`, and what follows it: its inverse, for exactly
/// the words `quoted` writes and no other spelling of them.
fn unquote(text: &str) -> Option<(String, &str)> {
    let (word, rest) = if let Some(mut rest) = text.strip_prefix('\'') {
        let mut word = String::new();
        loop {
            let end = rest.find('\'')?;
            word.push_str(&rest[..end]);
            rest = &rest[end + 1..];
            match rest.strip_prefix("\\''") {
                Some(more) => {
                    word.push('\'');
                    rest = more;
                }
                None => break (word, rest),
            }
        }
    } else {
        let end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || "/._-+".contains(c)))
            .unwrap_or(text.len());
        (text[..end].to_owned(), &text[end..])
    };
    (!word.is_empty() && quoted(&word) == text[..text.len() - rest.len()]).then_some((word, rest))
}

/// What `text` is when it is exactly how a command names rexboard: bare, a `$HOME` spelling, or an
/// absolute path as `quoted` writes it. The path, or `rexboard`, or the `$HOME` path as written.
fn spelling(text: &str) -> Option<String> {
    if text == "rexboard" || HOME_BINS.contains(&text) {
        return Some(text.trim_matches('"').to_owned());
    }
    let (path, rest) = unquote(text)?;
    (rest.is_empty() && path.starts_with('/') && Path::new(&path).file_name()? == "rexboard")
        .then_some(path)
}

/// The rexboard `command` runs, as written, when `command` is exactly a hook rexboard writes or
/// wrote for this harness and event, else None: `REXBOARD hook HARNESS EVENT` (v0.14), or inside
/// `/bin/sh -c '… || true'` (since), `REXBOARD` being one of the `spelling`s, or the portable
/// form. Nothing else is matched, so a command of the user's is never taken for rexboard's.
fn hook_bin(command: &str, harness: &str, event: &str) -> Option<String> {
    let call = format!(" hook {harness} {event}");
    if command
        == format!(
            "/bin/sh -c {}",
            quoted(&format!(
                "{PORTABLE}; \"$B\" hook {harness} {event} || true"
            ))
        )
    {
        return Some(HOME_BINS[0].to_owned());
    }
    match command.strip_prefix("/bin/sh -c ") {
        Some(rest) => {
            let (script, after) = unquote(rest)?;
            let bin = script.strip_suffix(&format!("{call} || true"))?;
            after.is_empty().then(|| spelling(bin)).flatten()
        }
        None => spelling(command.strip_suffix(&call)?),
    }
}

/// Whether `command` is the `rex-agent-status` bridge's own invocation, in a spelling it was
/// installed in: its name in a command of the user's is not it.
fn bridge(command: &str, event: &str) -> bool {
    const GUARDS: [&str; 3] = [
        "",
        r#"[ -n "$TMUX" ] || exit 0; "#,
        r#"[ -n "$TMUX$REX_BLOCK" ] || case "$TERM" in tmux*|screen*) ;; *) exit 0;; esac; "#,
    ];
    GUARDS.iter().any(|guard| {
        ["claude-code", "codex"].iter().any(|agent| {
            let call = format!(
                r#"/bin/sh -c '{guard}exec /usr/bin/python3 "$HOME/.local/bin/rex-agent-status" {agent}'"#
            );
            command == call || command == format!("{call} # {event}")
        })
    })
}

/// A hook of rexboard's own for this harness and event, by any path.
fn ours(hook: &Value, harness: &str, event: &str) -> bool {
    hook["command"]
        .as_str()
        .is_some_and(|c| hook_bin(c, harness, event).is_some())
}

/// A hook of rexboard's for this event that would run here: not an absolute path to a file this
/// host lacks or cannot execute (read from its mode; never run), so a stale path is still
/// repaired and one from another host is reported missing.
fn present(hook: &Value, harness: &str, event: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    hook["command"]
        .as_str()
        .and_then(|c| hook_bin(c, harness, event))
        .is_some_and(|bin| {
            !bin.starts_with('/')
                || std::fs::metadata(&bin)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// A hook install may replace or take out: rexboard's own, or the bridge it replaces.
fn reports(hook: &Value, harness: &str, event: &str) -> bool {
    ours(hook, harness, event) || hook["command"].as_str().is_some_and(|c| bridge(c, event))
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
        (Mode::Uninstall, None) => (stripped(&live, harness), "not installed"),
    };
    let mut changes = changes(&live, &after, harness, events);
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
    for (event, groups) in live["hooks"].as_object().into_iter().flatten() {
        let hooks = groups.as_array().into_iter().flatten();
        for hook in hooks.flat_map(|g| g["hooks"].as_array().into_iter().flatten()) {
            if let Some(found) = hook["command"]
                .as_str()
                .and_then(|c| hook_bin(c, harness, event))
            {
                bins.push(quoted(&found));
            }
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
        // A bridge hook stays gone: it exits 2, which blocks the agent, once its script is.
        let bridge = every_hook(&was).any(|hook| {
            hook["command"]
                .as_str()
                .is_some_and(|c| c.contains("rex-agent-status"))
        });
        let clean = was.is_object()
            && stripped(&was, harness) == was
            && unchained(&was["statusLine"]).is_none()
            && !bridge;
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

/// Every hook in a settings value, in each event's groups.
fn every_hook(settings: &Value) -> impl Iterator<Item = &Value> {
    settings["hooks"]
        .as_object()
        .into_iter()
        .flat_map(|hooks| hooks.values())
        .flat_map(|groups| groups.as_array().into_iter().flatten())
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
}

/// `live` without the hooks install added. A group, an event or `hooks` that only held them goes;
/// one the user left empty stays.
fn stripped(live: &Value, harness: &str) -> Value {
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
            list.retain(|hook| !ours(hook, harness, event));
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
    // Install's own line (the portable one too): rexboard alone.
    if let Some(bin) = command.strip_suffix(" usage claude") {
        let portable = format!("{PORTABLE}; \"$B\"");
        return (bin == portable || spelling(bin).is_some()).then_some(None);
    }
    // Chained: `GUARD && exec BIN usage claude -- /bin/sh -c THEIRS || exec /bin/sh -c THEIRS`,
    // GUARD being what each release wrote; THEIRS is the user's line as `quoted` writes it.
    let (guard, mine) = command.split_once(" && exec ")?;
    let (bin, tail) = mine.split_once(" usage claude -- /bin/sh -c ")?;
    let (theirs, rest) = unquote(tail)?;
    let (found, run) = (
        format!("{bin} usage --help >/dev/null 2>&1"),
        format!("{PORTABLE}; \"$B\" usage --help >/dev/null 2>&1"),
    );
    let guarded = if bin == "\"$B\"" {
        guard == run
    } else {
        spelling(bin).is_some()
            && (guard == found
                || guard == format!("[ -x {bin} ]")
                || (bin == "rexboard" && guard == "command -v rexboard >/dev/null 2>&1"))
    };
    (guarded && rest == format!(" || exec /bin/sh -c {}", quoted(&theirs))).then_some(Some(theirs))
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
        let has = hooks
            .get(&event)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
            .any(|h| present(h, harness, &event));
        for group in hooks
            .get(&event)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let all = group["hooks"].as_array().cloned().unwrap_or_default();
            // A rexboard hook that would run stays as it is; the rest that report go.
            let rest: Vec<Value> = all
                .iter()
                .filter(|h| !reports(h, harness, &event) || present(h, harness, &event))
                .cloned()
                .collect();
            if rest.len() < all.len() && !placed && !has {
                new.extend(want(&event));
                placed = true;
            }
            if !rest.is_empty() {
                let mut kept = group.clone();
                kept["hooks"] = Value::Array(rest);
                new.push(kept);
            }
        }
        if !placed && !has {
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
fn changes(live: &Value, after: &Value, harness: &str, events: &[Event]) -> Vec<String> {
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
            .filter(|hook| reports(hook, harness, event))
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
            let yours = hooks(after, event)
                .iter()
                .filter(|h| !reports(h, harness, event))
                .count();
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
        let said = changes(&live, &after, "claude", CLAUDE_EVENTS);
        assert!(
            said.contains(
                &"Stop: replace `rex-agent-status` with `/bin/sh -c 'rexboard hook claude Stop || true'`".to_owned()
            ) && said.contains(&"PreToolUse: add `/bin/sh -c 'rexboard hook claude PreToolUse || true'`".to_owned()),
            "{said:#?}"
        );
        assert!(changes(&after, &after, "claude", CLAUDE_EVENTS).is_empty());
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
            changes(&live, &after, "codex", CODEX_EVENTS)
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
            assert_eq!(
                hook_bin(&command, "claude", "Stop").as_deref(),
                Some(bin),
                "{command}"
            );
        }
        for theirs in [
            "afplay done.aiff",
            "/usr/local/bin/rexboard-notes hook",
            "/bin/sh -c 'echo rexboard hook'",
            "echo 'rexboard'",
        ] {
            assert!(
                !reports(&json!({ "command": theirs }), "claude", "Stop"),
                "{theirs}"
            );
        }
        assert!(
            reports(
                &json!({ "command": "rexboard hook claude Stop" }),
                "claude",
                "Stop"
            ),
            "the bare form"
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
        let out = stripped(&live, "claude");
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
            stripped(&only, "claude"),
            json!({ "model": "opus" }),
            "hooks only held rexboard's"
        );
    }

    #[test]
    fn only_the_hooks_install_writes_are_rexboards_to_take_out() {
        for (command, harness, event, bin) in [
            (
                hook_command("/opt/homebrew/bin/rexboard", "claude", "Stop"),
                "claude",
                "Stop",
                Some("/opt/homebrew/bin/rexboard"),
            ),
            (
                hook_command("'/tmp/My Tools/rexboard'", "codex", "Stop"),
                "codex",
                "Stop",
                Some("/tmp/My Tools/rexboard"),
            ),
            (
                "rexboard hook claude Stop".into(),
                "claude",
                "Stop",
                Some("rexboard"),
            ),
            (
                "/bin/sh -c 'rexboard hook claude Notification; afplay ping.aiff'".into(),
                "claude",
                "Notification",
                None,
            ),
            (
                "rexboard hook claude PreCompact && notify-send compacting".into(),
                "claude",
                "PreCompact",
                None,
            ),
            (
                "rexboard hook claude Stop || say done".into(),
                "claude",
                "Stop",
                None,
            ),
            ("rexboard usage claude".into(), "claude", "Stop", None),
            (
                "echo rexboard hook claude Stop".into(),
                "claude",
                "Stop",
                None,
            ),
            (
                hook_command("rexboard", "claude", "Stop"),
                "codex",
                "Stop",
                None,
            ),
        ] {
            assert_eq!(
                hook_bin(&command, harness, event).as_deref(),
                bin,
                "{command}"
            );
        }
    }

    /// The command a Codex hook can hold for each way a file may name rexboard: an absolute path
    /// that exists here, a bare name, a `$HOME` path and the portable form.
    fn forms(exists: &str) -> Vec<(&'static str, String)> {
        let portable =
            |event: &str| format!("/bin/sh -c '{PORTABLE}; \"$B\" hook codex {event} || true'");
        vec![
            (
                "absolute",
                format!("/bin/sh -c '{exists} hook codex EVENT || true'"),
            ),
            (
                "bare",
                "/bin/sh -c 'rexboard hook codex EVENT || true'".into(),
            ),
            (
                "home",
                "/bin/sh -c '$HOME/.local/bin/rexboard hook codex EVENT || true'".into(),
            ),
            ("portable", portable("EVENT")),
        ]
    }

    /// A `rexboard` in `dir` that can be executed.
    fn executable(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let file = dir.join("rexboard");
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        file
    }

    fn codex_file(dir: &Path, command: &str) -> PathBuf {
        let hooks: serde_json::Map<String, Value> = CODEX_EVENTS
            .iter()
            .map(|(event, _)| {
                let command = command.replace("EVENT", event);
                (
                    event.to_string(),
                    json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]),
                )
            })
            .collect();
        let file = dir.join("hooks.json");
        std::fs::write(&file, json!({ "hooks": hooks }).to_string()).unwrap();
        file
    }

    #[test]
    fn a_hook_is_present_by_any_path_and_in_the_portable_form_and_install_leaves_it() {
        let dir = tempfile::tempdir().unwrap();
        let here = executable(dir.path());
        for (name, command) in forms(&here.display().to_string()) {
            let file = codex_file(dir.path(), &command);
            let before = std::fs::read_to_string(&file).unwrap();
            // Status and install, from a binary that is somewhere else.
            let plan = settings(
                "codex",
                file.clone(),
                CODEX_EVENTS,
                false,
                "/elsewhere/bin/rexboard",
                Mode::Install,
            )
            .unwrap();
            assert_eq!(
                (plan.state, plan.changes.len()),
                ("installed", 0),
                "{name}: {:?}",
                plan.changes
            );
            assert!(plan.after.is_none(), "{name}: nothing is rewritten");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
            // Uninstall takes them all out.
            let plan = settings(
                "codex",
                file.clone(),
                CODEX_EVENTS,
                false,
                "/elsewhere/bin/rexboard",
                Mode::Uninstall,
            )
            .unwrap();
            assert_eq!(
                plan.changes.len(),
                CODEX_EVENTS.len(),
                "{name}: {:?}",
                plan.changes
            );
            assert!(matches!(plan.after, Some(Write::Remove)), "{name}");
        }
    }

    #[test]
    fn install_adds_only_the_events_that_lack_a_hook_and_keeps_the_others_as_written() {
        let portable = format!("/bin/sh -c '{PORTABLE}; \"$B\" hook codex Stop || true'");
        let live = json!({ "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": portable, "timeout": 5 }] }],
            "SessionStart": [{ "hooks": [{ "type": "command", "command": "mine" },
                { "type": "command", "command": "rexboard hook codex SessionStart" }] }],
        }});
        let after = merged(&live, "codex", CODEX_EVENTS, "/opt/rexboard");
        assert_eq!(
            after["hooks"]["Stop"], live["hooks"]["Stop"],
            "the portable hook stays"
        );
        assert_eq!(
            after["hooks"]["SessionStart"], live["hooks"]["SessionStart"],
            "so does a bare one"
        );
        assert_eq!(
            after["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "/bin/sh -c '/opt/rexboard hook codex PreToolUse || true'",
            "a missing event is added, with this binary's own path"
        );
        let said = changes(&live, &after, "codex", CODEX_EVENTS);
        assert_eq!(said.len(), 4, "{said:?}");
        assert!(said.iter().all(|c| c.contains(": add ")
            && !c.starts_with("Stop")
            && !c.starts_with("SessionStart")));
    }

    #[test]
    fn an_absolute_path_this_host_lacks_is_stale_and_install_repairs_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = codex_file(
            dir.path(),
            "/bin/sh -c '/opt/homebrew/bin/gone/rexboard hook codex EVENT || true'",
        );
        let plan = settings(
            "codex",
            file,
            CODEX_EVENTS,
            false,
            "/here/rexboard",
            Mode::Install,
        )
        .unwrap();
        assert_eq!(plan.state, "changes");
        assert_eq!(plan.changes.len(), CODEX_EVENTS.len(), "{:?}", plan.changes);
        // Uninstall still knows it for rexboard's.
        let live = serde_json::from_str::<Value>(
            &std::fs::read_to_string(dir.path().join("hooks.json")).unwrap(),
        )
        .unwrap();
        for (event, groups) in live["hooks"].as_object().unwrap() {
            let hook = &groups[0]["hooks"][0];
            assert!(
                ours(hook, "codex", event),
                "{event}: uninstall still knows it"
            );
            assert!(!present(hook, "codex", event), "{event}");
        }
    }

    #[test]
    fn only_the_whole_portable_form_is_rexboards_not_one_with_more_in_it() {
        let portable =
            |tail: &str| format!("/bin/sh -c '{PORTABLE}; \"$B\" hook codex Stop{tail}'");
        assert!(hook_bin(&portable(" || true"), "codex", "Stop").is_some());
        assert!(hook_bin(&portable(" || true"), "codex", "SessionEnd").is_none());
        for tail in [
            "",
            " || true; rm -rf x",
            " extra || true",
            " && other || true",
        ] {
            assert!(
                hook_bin(&portable(tail), "codex", "Stop").is_none(),
                "{tail:?}"
            );
        }
        assert_eq!(
            hook_bin(
                "/bin/sh -c 'B=/tmp/mine; \"$B\" hook codex Stop || true'",
                "codex",
                "Stop"
            ),
            None,
            "another assignment is not the portable form"
        );
    }

    #[test]
    fn the_status_line_in_the_portable_form_is_present_and_uninstall_unwinds_it() {
        let theirs = r#""$HOME/.local/bin/agent-statusline""#;
        let command = format!(
            "{PORTABLE}; \"$B\" usage --help >/dev/null 2>&1 && exec \"$B\" usage claude -- /bin/sh -c {} || exec /bin/sh -c {}",
            quoted(theirs),
            quoted(theirs)
        );
        let line = json!({ "type": "command", "command": command });
        assert_eq!(
            usage_line(&line, "/x/rexboard"),
            None,
            "present: install leaves it"
        );
        assert_eq!(
            unchained(&line),
            Some(Some(theirs.to_owned())),
            "uninstall puts theirs back"
        );
        let alone =
            json!({ "type": "command", "command": format!("{PORTABLE}; \"$B\" usage claude") });
        assert_eq!(unchained(&alone), Some(None));
        let absolute = json!({ "type": "command", "command": format!(
            "/opt/homebrew/bin/rexboard usage --help >/dev/null 2>&1 && exec /opt/homebrew/bin/rexboard usage claude -- /bin/sh -c {} || exec /bin/sh -c {}",
            quoted(theirs),
            quoted(theirs)
        )});
        assert_eq!(
            unchained(&absolute),
            Some(Some(theirs.to_owned())),
            "an absolute path too"
        );
    }

    fn hook(command: &str) -> Value {
        json!({ "type": "command", "command": command, "timeout": 5 })
    }

    /// Codex's events, each holding `holds` in `Stop` and its own hook everywhere else.
    fn stop_holding(holds: &[&str]) -> Value {
        let mut hooks = serde_json::Map::new();
        for (event, _) in CODEX_EVENTS {
            let own = hook_command("rexboard", "codex", event);
            let list: Vec<Value> = if *event == "Stop" {
                holds.iter().map(|c| hook(c)).collect()
            } else {
                vec![hook(&own)]
            };
            hooks.insert(event.to_string(), json!([{ "hooks": list }]));
        }
        json!({ "hooks": hooks })
    }

    fn stop_commands(live: &Value) -> Vec<String> {
        live["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_hook_for_another_event_or_harness_does_not_satisfy_this_one_and_is_left_alone() {
        let own = hook_command("rexboard", "codex", "Stop");
        let portable =
            |event: &str| format!("/bin/sh -c '{PORTABLE}; \"$B\" hook codex {event} || true'");
        for wrong in [
            hook_command("rexboard", "codex", "SessionStart"),
            hook_command("rexboard", "claude", "Stop"),
            portable("SessionEnd"),
        ] {
            let live = stop_holding(&[&wrong]);
            let after = merged(&live, "codex", CODEX_EVENTS, "rexboard");
            assert_eq!(
                stop_commands(&after),
                [wrong.clone(), own.clone()],
                "added beside: {wrong}"
            );
        }
        let live = stop_holding(&[&portable("Stop")]);
        assert_eq!(
            merged(&live, "codex", CODEX_EVENTS, "rexboard"),
            live,
            "its own event is present"
        );
    }

    #[test]
    fn only_a_whole_command_equal_to_a_spelling_rexboard_wrote_is_rexboards() {
        let run = |script: &str| format!("/bin/sh -c {}", quoted(script));
        let broken = [
            // `$HOME` stays literal in single quotes.
            run("'$HOME/.local/bin/rexboard' hook codex Stop || true"),
            // The outer shell expands `$B` before the script assigns it.
            r#"/bin/sh -c "B=$HOME/.local/bin/rexboard; [ -x \"$B\" ] || B=/opt/homebrew/bin/rexboard; \"$B\" hook codex Stop || true""#.to_owned(),
            run("\"$(echo rexboard)\" hook codex Stop || true"),
            // A quoted or escaped `||` is an argument, not the operator.
            run("rexboard hook codex Stop '||' true"),
            run("rexboard hook codex Stop \\|\\| true"),
            // An unterminated script and an unquoted path with a quote in it.
            "/bin/sh -c 'rexboard hook codex Stop || true".to_owned(),
            format!("/bin/sh -c '{PORTABLE}; \"$B\" hook codex Stop || true"),
            run("\"/tmp/it's $VAR/rexboard\" hook codex Stop || true"),
            run("'/tmp/it'\"'\"'s $VAR/rexboard' hook codex Stop || true"),
            // Another program with rexboard in its name, and a relative path.
            run("/tmp/not-rexboard hook codex Stop || true"),
            run("bin/rexboard hook codex Stop || true"),
        ];
        for command in &broken {
            assert!(hook_bin(command, "codex", "Stop").is_none(), "{command}");
        }
        for command in [
            run("$HOME/.local/bin/rexboard hook codex Stop || true"),
            run("\"$HOME/.local/bin/rexboard\" hook codex Stop || true"),
            hook_command(&quoted("/opt/it's/rexboard"), "codex", "Stop"),
            hook_command("rexboard", "codex", "Stop"),
            "rexboard hook codex Stop".to_owned(),
        ] {
            assert!(hook_bin(&command, "codex", "Stop").is_some(), "{command}");
        }
    }

    #[test]
    fn the_bridges_name_in_a_command_of_the_users_is_not_the_bridge() {
        let live_with = |theirs: &str| {
            let mut live = stop_holding(&[theirs]);
            live["hooks"]["Stop"][0]["hooks"][0]["command"] = json!(theirs);
            live
        };
        let data = "printf '%s' 'rex-agent-status is retired'";
        let kept = merged(&live_with(data), "codex", CODEX_EVENTS, "rexboard");
        assert_eq!(
            stop_commands(&kept),
            [data.to_owned(), hook_command("rexboard", "codex", "Stop")],
            "kept, with rexboard's own beside it"
        );
        let real = r#"/bin/sh -c '[ -n "$TMUX$REX_BLOCK" ] || case "$TERM" in tmux*|screen*) ;; *) exit 0;; esac; exec /usr/bin/python3 "$HOME/.local/bin/rex-agent-status" codex'"#;
        let replaced = merged(&live_with(real), "codex", CODEX_EVENTS, "rexboard");
        assert_eq!(
            stop_commands(&replaced),
            [hook_command("rexboard", "codex", "Stop")],
            "the bridge's own command is replaced"
        );
    }

    #[test]
    fn an_absolute_rexboard_that_cannot_be_executed_is_not_present_and_is_repaired() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = executable(dir.path());
        let command = hook_command(&file.display().to_string(), "codex", "Stop");
        let live = stop_holding(&[&command]);
        assert_eq!(
            merged(&live, "codex", CODEX_EVENTS, "/new/rexboard"),
            live,
            "executable: left"
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let after = merged(&live, "codex", CODEX_EVENTS, "/new/rexboard");
        assert_eq!(
            stop_commands(&after),
            [hook_command("/new/rexboard", "codex", "Stop")],
            "mode 0644: replaced"
        );
    }

    #[test]
    fn a_users_composite_that_starts_with_rexboards_command_is_never_removed() {
        let own = hook_command("rexboard", "codex", "Stop");
        for theirs in [
            "rexboard hook codex Stop || true; printf user-work",
            "/bin/sh -c 'rexboard hook codex Stop || true; printf user-work'",
            "rexboard hook codex Stop && notify-send done",
        ] {
            // Alone, the event is missing its own hook, so one is added beside theirs.
            let live = stop_holding(&[theirs]);
            let after = merged(&live, "codex", CODEX_EVENTS, "rexboard");
            assert_eq!(
                stop_commands(&after),
                [theirs.to_owned(), own.clone()],
                "{theirs}"
            );
            // With its own already there, nothing changes.
            let live = stop_holding(&[&own, theirs]);
            assert_eq!(
                merged(&live, "codex", CODEX_EVENTS, "rexboard"),
                live,
                "{theirs}"
            );
            // Uninstall leaves theirs too.
            assert!(
                stripped(&live, "claude")["hooks"]["Stop"][0]["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|h| h["command"] == theirs)
            );
        }
    }

    #[test]
    fn a_status_line_wrapped_with_rexboard_as_the_user_spelled_it_is_restored_as_they_wrote_it() {
        for theirs in [
            r#""$HOME/.local/bin/agent-statusline""#,
            "echo it's \"$HOME\" costs $5",
            "printf one \\\ntwo",
        ] {
            for bin in [
                "rexboard",
                "$HOME/.local/bin/rexboard",
                "\"$HOME/.local/bin/rexboard\"",
            ] {
                let theirs_line = quoted(theirs);
                let command = format!(
                    "{bin} usage --help >/dev/null 2>&1 && exec {bin} usage claude -- /bin/sh -c {theirs_line} || exec /bin/sh -c {theirs_line}"
                );
                let line = json!({ "type": "command", "command": command });
                assert_eq!(usage_line(&line, "/x/rexboard"), None, "{command}");
                assert_eq!(unchained(&line), Some(Some(theirs.to_owned())), "{command}");
            }
        }
        // Not rexboard's: a single-quoted `$HOME`, or a different program.
        for bin in ["'$HOME/.local/bin/rexboard'", "/usr/bin/other"] {
            let command = format!("{bin} usage claude");
            assert_eq!(unchained(&json!({ "command": command })), None, "{bin}");
        }
    }

    #[test]
    fn the_status_lines_every_release_wrote_are_unwound_to_the_users_own_command() {
        let theirs = "echo it's \"$HOME\" costs $5";
        let t = quoted(theirs);
        for bin in [
            "rexboard".to_owned(),
            quoted("/opt/homebrew/bin/rexboard"),
            quoted("/tmp/My Tools/rexboard"),
            quoted("/tmp/it's/rexboard"),
            quoted("/tmp/a \"b\" $c/rexboard"),
            quoted("/tmp/ünï/rexboard"),
        ] {
            // v0.14.0 guarded with `command -v` (bare) or `[ -x BIN ]`; later ones with `usage --help`.
            let guards = [
                if bin == "rexboard" {
                    "command -v rexboard >/dev/null 2>&1".to_owned()
                } else {
                    format!("[ -x {bin} ]")
                },
                format!("{bin} usage --help >/dev/null 2>&1"),
            ];
            for guard in guards {
                let command = format!(
                    "{guard} && exec {bin} usage claude -- /bin/sh -c {t} || exec /bin/sh -c {t}"
                );
                let line = json!({ "type": "command", "command": command });
                assert_eq!(unchained(&line), Some(Some(theirs.to_owned())), "{command}");
                assert_eq!(usage_line(&line, "/x/rexboard"), None, "present: {command}");
            }
            let alone = json!({ "command": format!("{bin} usage claude") });
            assert_eq!(unchained(&alone), Some(None), "{bin}");
        }
        // A user's own line that only looks like one is not unwound.
        for command in [
            format!(
                "[ -x /bin/ls ] && exec /bin/ls usage claude -- /bin/sh -c {t} || exec /bin/sh -c {t}"
            ),
            format!(
                "command -v rexboard >/dev/null 2>&1 && exec rexboard usage claude -- /bin/sh -c {t} || exec /bin/sh -c 'other'"
            ),
            "rexboard usage claude; rm -rf x".to_owned(),
        ] {
            assert_eq!(unchained(&json!({ "command": command })), None, "{command}");
        }
    }
}
