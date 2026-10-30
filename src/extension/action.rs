//! Extension actions: a named argv template the user starts with a key, a typed line and a
//! confirm. rexboard owns the prompt, the checks and the run; the extension only declares it.

use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runner::{exited, kill_group};
use super::{field_id, is_name};
use crate::board::Row;

/// The most bytes of text an action may take, whatever its manifest says.
pub const MAX_TEXT: usize = 4096;
/// The most of an action's stdout or stderr that is kept.
const KEEP: u64 = 4096;

/// One `[[actions]]` entry of a manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub name: String,
    /// The input line's label and the key table's word.
    pub title: String,
    /// One character, typed after the board's `x`.
    pub key: String,
    /// argv, no shell; a leading `./` is the extension's directory.
    pub cmd: Vec<String>,
    /// What the confirm line says the action does and to whom, with `cmd`'s placeholders but not
    /// `{text}`: the line always adds the text. Empty is the title.
    #[serde(default)]
    pub confirm: String,
    /// Fields of this extension a row must have a value for.
    #[serde(default)]
    pub when: Vec<String>,
    /// Bytes of text, at most MAX_TEXT.
    #[serde(default = "max")]
    pub max: usize,
    /// Seconds before the run and what it started are killed.
    #[serde(default = "timeout")]
    pub timeout: u64,
}

fn max() -> usize {
    1000
}

fn timeout() -> u64 {
    10
}

/// An action as the board gets it from serve: what to run, where, and with which settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offered {
    pub extension: String,
    pub dir: PathBuf,
    #[serde(default)]
    pub settings: Value,
    pub action: Action,
}

/// What to run for one row and one line of text, once every check has passed.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub argv: Vec<String>,
    /// What the action does and to whom: the manifest's `confirm`, or its title.
    pub confirm: String,
    /// The typed line, which every confirm shows beside `confirm`.
    pub text: String,
    pub dir: PathBuf,
    pub timeout: Duration,
    /// `REXBOARD_EXTENSION`, `REXBOARD_ACTION` and `REXBOARD_SETTINGS`.
    pub env: Vec<(String, String)>,
}

impl Plan {
    /// The confirm line as the CLI shows it: the target, then the text in quotes. The board
    /// draws the same two parts, cutting the target first.
    pub fn line(&self) -> String {
        format!("{}: \"{}\"", self.confirm, self.text)
    }
}

/// The placeholders a template may hold.
#[derive(Debug, PartialEq)]
enum Part<'a> {
    Text(&'a str),
    Host,
    Typed,
    Field(&'a str),
}

fn parts(template: &str) -> Result<Vec<Part<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            out.push(Part::Text(&rest[..open]));
        }
        let close = rest[open..]
            .find('}')
            .ok_or_else(|| format!("{template:?} has a `{{` with no `}}`"))?
            + open;
        out.push(match &rest[open + 1..close] {
            "host" => Part::Host,
            "text" => Part::Typed,
            name => match name.strip_prefix("field:") {
                Some(field) => Part::Field(field),
                None => {
                    return Err(format!(
                        "{template:?} has the unknown placeholder {{{name}}}"
                    ));
                }
            },
        });
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        out.push(Part::Text(rest));
    }
    Ok(out)
}

impl Action {
    /// What is wrong with the declaration, given the field names the manifest declares.
    pub fn check(&self, declared: &BTreeSet<&str>) -> Result<(), String> {
        let at = |what: &str| format!("action {:?}: {what}", self.name);
        if !is_name(&self.name) {
            return Err(at("the name must be [a-z0-9-]+"));
        }
        if self.title.trim().is_empty() {
            return Err(at("a title is needed"));
        }
        let mut key = self.key.chars();
        if key.next().is_none_or(|c| c.is_control() || c == ' ') || key.next().is_some() {
            return Err(at("the key must be one printable character"));
        }
        if self.cmd.first().is_none_or(String::is_empty) {
            return Err(at("cmd must name a program"));
        }
        if !(1..=MAX_TEXT).contains(&self.max) {
            return Err(at(&format!(
                "max is {}; it must be 1 to {MAX_TEXT}",
                self.max
            )));
        }
        if !(1..=60).contains(&self.timeout) {
            return Err(at(&format!(
                "timeout is {}s; it must be 1 to 60",
                self.timeout
            )));
        }
        if let Some(field) = self.when.iter().find(|f| !declared.contains(f.as_str())) {
            return Err(at(&format!(
                "when names {field:?}, which the manifest does not declare"
            )));
        }
        for (i, template) in self.cmd.iter().chain([&self.confirm]).enumerate() {
            for part in parts(template).map_err(|why| at(&why))? {
                match part {
                    Part::Field(field) if !declared.contains(field) => {
                        return Err(at(&format!("{{field:{field}}} is not a declared field")));
                    }
                    Part::Typed if i >= self.cmd.len() => {
                        return Err(at(
                            "{text} is for cmd; every confirm line shows the text itself",
                        ));
                    }
                    _ if i == 0 && !matches!(part, Part::Text(_)) => {
                        return Err(at("the program cannot be a placeholder"));
                    }
                    _ => {}
                }
            }
        }
        if !self.cmd.iter().any(|t| t.contains("{text}")) {
            return Err(at("cmd must hold {text}, the line the user types"));
        }
        Ok(())
    }

    /// Whether `row` has a value for every field `when` names.
    fn applies(&self, extension: &str, row: &Row) -> bool {
        self.when.iter().all(|f| {
            row.ext
                .get(&field_id(extension, f))
                .is_some_and(|v| !v.is_empty())
        })
    }
}

impl Offered {
    pub fn applies(&self, row: &Row) -> bool {
        self.action.applies(&self.extension, row)
    }

    /// The run for `row` and `text`: argv as one element per template element, with the text
    /// whole in its element, whatever it holds.
    pub fn plan(&self, row: &Row, text: &str) -> Result<Plan, String> {
        let action = &self.action;
        if !self.applies(row) {
            return Err(format!(
                "{} does not apply to this row: it has no {}",
                action.title,
                action.when.join(", ")
            ));
        }
        let text = valid_text(text, action.max)?;
        let expand = |template: &str| -> Result<String, String> {
            let mut out = String::new();
            for part in parts(template)? {
                match part {
                    Part::Text(s) => out.push_str(s),
                    Part::Host => out.push_str(&row.pane.host),
                    Part::Typed => out.push_str(text),
                    Part::Field(f) => match row.ext.get(&field_id(&self.extension, f)) {
                        Some(v) if !v.is_empty() => out.push_str(v),
                        _ => return Err(format!("this row has no {f}")),
                    },
                }
            }
            Ok(out)
        };
        let mut argv = action
            .cmd
            .iter()
            .map(|t| expand(t))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(found) = program(&self.dir, &argv[0]) {
            argv[0] = found.display().to_string();
        } else if argv[0].contains('/') {
            let rest = argv[0].strip_prefix("./").unwrap_or(&argv[0]);
            argv[0] = self.dir.join(rest).display().to_string();
        }
        let confirm = if action.confirm.is_empty() {
            action.title.clone()
        } else {
            expand(&action.confirm)?
        };
        Ok(Plan {
            argv,
            confirm,
            text: text.to_owned(),
            dir: self.dir.clone(),
            timeout: Duration::from_secs(action.timeout),
            env: vec![
                ("REXBOARD_EXTENSION".into(), self.extension.clone()),
                ("REXBOARD_ACTION".into(), action.name.clone()),
                ("REXBOARD_SETTINGS".into(), self.settings.to_string()),
            ],
        })
    }
}

/// One line of at most `max` bytes with no control character.
fn valid_text(text: &str, max: usize) -> Result<&str, String> {
    if text.trim().is_empty() {
        return Err("there is no text to send".to_owned());
    }
    if text.len() > max {
        return Err(format!(
            "the text is {} bytes; the most is {max}",
            text.len()
        ));
    }
    if text.chars().any(char::is_control) {
        return Err("the text is one line with no control characters".to_owned());
    }
    Ok(text)
}

/// The first line a pipe says, sent once it closes. The rest is read and dropped, so the action
/// never blocks on a full pipe.
fn first_line(mut pipe: impl Read + Send + 'static) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = pipe.by_ref().take(KEEP).read_to_end(&mut kept);
        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        let line = String::from_utf8_lossy(&kept)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default()
            .to_owned();
        let _ = tx.send(line);
    });
    rx
}

/// Set by a signal handler once `stop_on_signals` is on: a run in flight kills its group and ends.
static CANCELLED: AtomicBool = AtomicBool::new(false);

extern "C" fn cancel(_: libc::c_int) {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// SIGINT, SIGTERM and SIGHUP stop a run and what it started, instead of ending the process
/// around it: the action is in its own group, so the terminal's Ctrl-C does not reach it.
pub fn stop_on_signals() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        unsafe { libc::signal(signal, cancel as *const () as libc::sighandler_t) };
    }
}

/// Runs `plan` in its directory, in its own process group, and returns the first line of what it
/// printed. A failure says its exit, or that it timed out or could not start.
pub fn run(plan: &Plan) -> Result<String, String> {
    let mut cmd = Command::new(&plan.argv[0]);
    cmd.args(&plan.argv[1..])
        .current_dir(&plan.dir)
        .envs(plan.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = crate::rex::spawn(&mut cmd)
        .map_err(|err| format!("could not run {}: {err}", plan.argv[0]))?;
    let pgid = child.id() as i32;
    let out = first_line(child.stdout.take().expect("piped stdout"));
    let err = first_line(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + plan.timeout;
    let outcome = loop {
        if exited(pgid) {
            kill_group(pgid);
            break child.wait().map_err(|e| e.to_string());
        }
        let late = Instant::now() >= deadline;
        if late || CANCELLED.load(Ordering::SeqCst) {
            kill_group(pgid);
            let _ = child.wait();
            break Err(if late {
                format!(
                    "timed out after {}s and was stopped",
                    plan.timeout.as_secs()
                )
            } else {
                "interrupted and was stopped".to_owned()
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let status = outcome?;
    // What something that left the group still holds open is not waited for past the deadline.
    let wait = deadline.saturating_duration_since(Instant::now());
    let (out, err) = (
        out.recv_timeout(wait).unwrap_or_default(),
        err.recv_timeout(wait).unwrap_or_default(),
    );
    if status.success() {
        return Ok(out);
    }
    let said = if err.is_empty() { out } else { err };
    Err(match status.code() {
        Some(code) if said.is_empty() => format!("exited with {code}"),
        Some(code) => format!("exited with {code}: {said}"),
        None => format!("stopped by a signal ({status})"),
    })
}

/// The file `first`, a `cmd`'s program, runs as, found the way exec finds it from `dir`: a path
/// with a slash is taken from `dir` unless it is absolute, and a bare name is the first
/// executable file of that name on PATH. None when there is none.
pub fn program(dir: &Path, first: &str) -> Option<PathBuf> {
    program_on(dir, first, &std::env::var_os("PATH").unwrap_or_default())
}

fn program_on(dir: &Path, first: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if first.contains('/') {
        vec![dir.join(first.strip_prefix("./").unwrap_or(first))]
    } else {
        std::env::split_paths(path).map(|d| d.join(first)).collect()
    };
    candidates.into_iter().find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::tests::{manifest, write};
    use std::path::Path;

    /// A manifest with `session` and `rig` fields and the action table `action`.
    fn with_action(action: &str) -> String {
        format!(
            "{}[[actions]]\nname = \"send\"\ntitle = \"Send\"\nkey = \"s\"\n{action}",
            manifest("fake", &["session", "rig"])
        )
    }

    fn load(text: &str) -> Result<crate::extension::Extension, String> {
        let root = tempfile::tempdir().unwrap();
        let dir = write(root.path(), "fake", text);
        crate::extension::load(&dir)
    }

    fn offered(extra: &str) -> Offered {
        let text = with_action(&format!(
            "cmd = [\"./send\", \"--host\", \"{{host}}\", \"--\", \"{{field:session}}\", \"{{text}}\"]\nwhen = [\"session\"]\n{extra}"
        ));
        let ext = load(&text).unwrap();
        Offered {
            extension: "fake".into(),
            dir: PathBuf::from("/ext"),
            settings: serde_json::json!({ "wait": true }),
            action: ext.manifest.actions[0].clone(),
        }
    }

    fn seat() -> Row {
        let mut row: Row = serde_json::from_value(serde_json::json!({
            "key": "mini/seat-a", "host": "mini", "session_id": "session:api", "session": "api",
            "window_id": "window:main", "tab": "main", "block_id": "seat-a", "pane": "seat-a",
            "title": "", "harness": "claude-code", "state": "blocked", "kind": null,
            "msg": "", "updated_at": "2026-01-01T00:00:00Z", "source": "native",
            "children": {}, "since": "2026-01-01T00:00:00.000Z", "since_estimated": false,
            "turn_started": null, "last_turn": null, "seen": false, "stale": false,
            "suspect": false, "suspect_reason": null, "group": "needs_you"
        }))
        .unwrap();
        row.ext.insert("fake.session".into(), "lead@api".into());
        row
    }

    #[test]
    fn a_good_action_loads_with_its_defaults() {
        let ext = load(&with_action(
            "cmd = [\"./send\", \"{text}\"]\nwhen = [\"session\"]\n",
        ))
        .unwrap();
        let action = &ext.manifest.actions[0];
        assert_eq!((action.max, action.timeout), (1000, 10));
        assert_eq!(action.when, ["session"]);
    }

    #[test]
    fn a_bad_action_says_what_is_wrong_with_it() {
        let bad = |action: &str, why: &str| {
            let err = load(&with_action(action)).unwrap_err();
            assert!(err.contains(why), "{action:?} gave {err:?}, not {why:?}");
        };
        bad("cmd = []\n", "cmd must name a program");
        bad(
            "cmd = [\"{text}\"]\n",
            "the program cannot be a placeholder",
        );
        bad("cmd = [\"./s\"]\n", "cmd must hold {text}");
        bad(
            "cmd = [\"./s\", \"{nope}\"]\n",
            "unknown placeholder {nope}",
        );
        bad("cmd = [\"./s\", \"{host\"]\n", "no `}`");
        bad(
            "cmd = [\"./s\", \"{field:gone}\"]\n",
            "{field:gone} is not a declared field",
        );
        bad(
            "cmd = [\"./s\"]\nwhen = [\"gone\"]\n",
            "when names \"gone\"",
        );
        bad("cmd = [\"./s\"]\nmax = 4097\n", "max is 4097");
        bad("cmd = [\"./s\"]\nmax = 0\n", "max is 0");
        bad("cmd = [\"./s\"]\ntimeout = 61\n", "timeout is 61s");
        bad("cmd = [\"./s\"]\ntimeout = 0\n", "timeout is 0s");
        bad("cmd = [\"./s\"]\nshell = true\n", "unknown field");
        let key =
            with_action("cmd = [\"./s\", \"{text}\"]\n").replace("key = \"s\"", "key = \"sx\"");
        assert!(load(&key).unwrap_err().contains("one printable character"));
        let twice = format!(
            "{}{}",
            with_action("cmd = [\"./s\", \"{text}\"]\n"),
            "[[actions]]\nname = \"other\"\ntitle = \"O\"\nkey = \"s\"\ncmd = [\"./s\", \"{text}\"]\n"
        );
        assert!(
            load(&twice)
                .unwrap_err()
                .contains("the key \"s\" is already")
        );
        let same = format!(
            "{}{}",
            with_action("cmd = [\"./s\", \"{text}\"]\n"),
            "[[actions]]\nname = \"send\"\ntitle = \"O\"\nkey = \"t\"\ncmd = [\"./s\", \"{text}\"]\n"
        );
        assert!(load(&same).unwrap_err().contains("declared twice"));
    }

    #[test]
    fn the_text_is_one_argv_element_whatever_it_holds() {
        let offered = offered("");
        for text in [
            "plain words",
            "it's \"quoted\" and `ticked`",
            "a; touch pwn",
            "$(touch pwn) ${HOME} $HOME",
            "a && b | c > d",
            "-rf --help",
            "*",
            "héllo wörld 🙂 日本語",
            " leading and trailing ",
        ] {
            let plan = offered.plan(&seat(), text).unwrap();
            assert_eq!(
                plan.argv,
                ["/ext/send", "--host", "mini", "--", "lead@api", text],
                "{text:?}"
            );
        }
    }

    #[test]
    fn control_characters_and_empty_text_are_refused() {
        let offered = offered("");
        for text in [
            "a\nb",
            "a\rb",
            "a\tb",
            "a\u{7}b",
            "\u{1b}[31mred",
            "a\u{0}b",
            "a\u{85}b",
        ] {
            let err = offered.plan(&seat(), text).unwrap_err();
            assert_eq!(
                err, "the text is one line with no control characters",
                "{text:?}"
            );
        }
        for text in ["", "   "] {
            assert_eq!(
                offered.plan(&seat(), text).unwrap_err(),
                "there is no text to send"
            );
        }
    }

    #[test]
    fn the_size_cap_counts_bytes() {
        let offered = offered("max = 10\n");
        assert!(offered.plan(&seat(), "0123456789").is_ok());
        let err = offered.plan(&seat(), "01234567890").unwrap_err();
        assert_eq!(err, "the text is 11 bytes; the most is 10");
        let err = offered.plan(&seat(), "éééééé").unwrap_err();
        assert_eq!(err, "the text is 12 bytes; the most is 10");
    }

    #[test]
    fn a_row_without_the_field_gets_no_action() {
        let offered = offered("");
        let mut bare = seat();
        bare.ext.clear();
        assert!(!offered.applies(&bare) && offered.applies(&seat()));
        assert!(
            offered
                .plan(&bare, "hi")
                .unwrap_err()
                .contains("does not apply")
        );
        bare.ext.insert("fake.session".into(), String::new());
        assert!(!offered.applies(&bare), "an empty value is no value");
    }

    #[test]
    fn every_confirm_line_names_the_target_and_quotes_the_text() {
        let plan = offered("").plan(&seat(), "hi there").unwrap();
        assert_eq!(
            (plan.confirm.as_str(), plan.line().as_str()),
            ("Send", "Send: \"hi there\"")
        );
        // A template that leaves the text out still shows it: the line is the board's, not the
        // manifest's.
        let plan = offered("confirm = \"send to {field:session} on {host}\"\n")
            .plan(&seat(), "say \"hi\"")
            .unwrap();
        assert_eq!(plan.line(), "send to lead@api on mini: \"say \"hi\"\"");
        assert_eq!(plan.text, "say \"hi\"");
        assert_eq!(plan.env[0], ("REXBOARD_EXTENSION".into(), "fake".into()));
        assert_eq!(plan.env[1], ("REXBOARD_ACTION".into(), "send".into()));
        assert_eq!(
            plan.env[2],
            ("REXBOARD_SETTINGS".into(), "{\"wait\":true}".into())
        );
    }

    #[test]
    fn a_confirm_that_takes_the_text_is_refused() {
        let err = load(&with_action(
            "cmd = [\"./s\", \"{text}\"]\nconfirm = \"send {text}\"\n",
        ))
        .unwrap_err();
        assert!(
            err.contains("{text} is for cmd; every confirm line shows the text itself"),
            "{err}"
        );
    }

    /// A plan that runs the shell `script` in `dir`.
    fn plan(dir: &Path, script: &str, timeout: u64) -> Plan {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("act");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        Plan {
            argv: vec![
                path.display().to_string(),
                "two words".into(),
                "$(touch pwn)".into(),
            ],
            confirm: String::new(),
            text: String::new(),
            dir: dir.to_owned(),
            timeout: Duration::from_secs(timeout),
            env: vec![("REXBOARD_ACTION".into(), "send".into())],
        }
    }

    #[test]
    fn a_run_gets_its_argv_whole_and_its_env_and_runs_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let script =
            "printf '%s\\n' \"$@\" > argv; echo \"$REXBOARD_ACTION\" > env; echo sent; echo more";
        let out = run(&plan(dir.path(), script, 5)).unwrap();
        assert_eq!(out, "sent");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("argv")).unwrap(),
            "two words\n$(touch pwn)\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("env")).unwrap(),
            "send\n"
        );
        assert!(!dir.path().join("pwn").exists());
    }

    #[test]
    fn a_failed_run_says_how_and_why() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(&plan(dir.path(), "echo boom >&2; echo out; exit 3", 5)).unwrap_err();
        assert_eq!(err, "exited with 3: boom");
        let err = run(&plan(dir.path(), "exit 4", 5)).unwrap_err();
        assert_eq!(err, "exited with 4");
        let mut missing = plan(dir.path(), "", 5);
        missing.argv[0] = dir.path().join("nope").display().to_string();
        assert!(run(&missing).unwrap_err().starts_with("could not run"));
    }

    #[test]
    fn a_run_that_outlasts_its_timeout_is_killed_with_what_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let script = "sleep 60 &\necho $! > child\nwait";
        let started = Instant::now();
        let err = run(&plan(dir.path(), script, 1)).unwrap_err();
        assert_eq!(err, "timed out after 1s and was stopped");
        assert!(started.elapsed() < Duration::from_secs(4));
        let child: i32 = std::fs::read_to_string(dir.path().join("child"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut gone = false;
        for _ in 0..100 {
            // kill -0 on a process that is gone, or one waiting to be reaped as a zombie.
            if unsafe { libc::kill(child, 0) } != 0 {
                gone = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(gone, "the sleep the action started outlived it");
    }

    #[test]
    fn the_program_is_found_as_exec_finds_it_from_the_extension_directory() {
        use std::ffi::OsString;
        let root = tempfile::tempdir().unwrap();
        let make = |rel: &str, mode: u32| {
            let path = root.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let ext = root.path().join("ext");
        let (first, second, plain) = (
            make("a/tool", 0o755),
            make("b/tool", 0o755),
            make("c/tool", 0o644),
        );
        let relative = make("ext/bin/send", 0o755);
        // A path with a slash is the extension's, whatever the caller's directory is.
        assert_eq!(
            program_on(&ext, "bin/send", &OsString::new()),
            Some(relative.clone())
        );
        assert_eq!(
            program_on(&ext, "./bin/send", &OsString::new()),
            Some(ext.join("bin/send"))
        );
        assert_eq!(
            program_on(&ext, relative.to_str().unwrap(), &OsString::new()),
            Some(relative)
        );
        assert_eq!(program_on(&ext, "bin/none", &OsString::new()), None);
        // A bare name is the first executable on PATH: a plain file earlier is skipped.
        let path = std::env::join_paths([
            root.path().join("c"),
            root.path().join("b"),
            root.path().join("a"),
        ])
        .unwrap();
        assert_eq!(program_on(&ext, "tool", &path), Some(second));
        let path = std::env::join_paths([root.path().join("a"), root.path().join("b")]).unwrap();
        assert_eq!(program_on(&ext, "tool", &path), Some(first));
        assert_eq!(
            program_on(&ext, "tool", &root.path().join("c").into_os_string()),
            None
        );
        assert!(plain.exists());
    }

    #[test]
    fn a_noisy_run_is_capped_and_never_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let out = run(&plan(dir.path(), "yes abcdefgh | head -c 3000000", 10)).unwrap();
        assert_eq!(out, "abcdefgh");
    }
}
