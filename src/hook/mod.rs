//! `rexboard hook`: an agent's hooks tell Rex its status. Rex learns it from OSC 7501, which
//! Claude Code and Codex send on their own straight in a Rex pane; inside tmux the sequence
//! stops at tmux, so the hook wraps it in tmux passthrough and writes it to the agent's pane.
//! It never fails the hook it runs in.

pub mod install;
pub mod report;

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::Result;
use report::{fields, osc, turn_record, wrap};

/// `rexboard hook HARNESS [EVENT]`, with the hook's JSON on stdin. EVENT fills in
/// `hook_event_name` when the JSON has none.
pub fn run(name: &str, event: Option<&str>) -> Result<()> {
    let harness = crate::harness::usage::named(name)?;
    let mut input = Vec::new();
    std::io::stdin().take(1 << 20).read_to_end(&mut input)?;
    let parsed: Option<Value> = if input.iter().all(u8::is_ascii_whitespace) {
        Some(Value::Object(Default::default()))
    } else {
        serde_json::from_slice::<Value>(&input).ok()
    };
    let Some(mut payload) = parsed else {
        log(None, None, "skipped: not JSON");
        return Ok(());
    };
    if !payload.is_object() {
        payload = Value::Object(Default::default());
    }
    if let Some(event) = event
        && payload["hook_event_name"].is_null()
    {
        payload["hook_event_name"] = event.into();
    }
    let name = payload["hook_event_name"].as_str().map(str::to_owned);
    if harness
        .usage_hook()
        .is_some_and(|hook| Some(hook) == name.as_deref())
    {
        crate::harness::usage::keep(harness, &payload);
    }
    tell(harness.id(), name.as_deref(), &payload);
    Ok(())
}

/// Writes the status to the agent's pane: wrapped for tmux and kept on the tmux pane for a
/// client that attaches later, or straight to the agent's tty in a Rex pane where the agent does
/// not report its own.
fn tell(app: &str, name: Option<&str>, event: &Value) {
    let pane = seat_pane();
    if pane.is_none() && env("REX_BLOCK").is_none() {
        return log(name, None, "skipped: not in tmux or Rex");
    }
    let Some(status) = report::status(event, app) else {
        return log(name, pane.as_deref(), "skipped: no status for this event");
    };
    let pairs = || fields(app, status.state, status.kind, status.msg.as_deref(), None);
    let Some(pane) = pane else {
        match app {
            "codex" => log(name, None, "skipped: codex reports to a Rex pane itself"),
            // pi reports nothing itself, so in a Rex pane the status goes straight to its tty.
            "pi" => match agent_tty() {
                Some((_, tty)) => {
                    write(&tty, &osc(&pairs()));
                    log(
                        name,
                        None,
                        &format!("wrote {} to the Rex pane", status.state),
                    );
                }
                None => log(name, None, "skipped: no tty"),
            },
            // Claude reports its own state; its final message goes to a `turn` record.
            _ => {
                let record = name.and_then(|name| turn_record(app, name, &status));
                let tty = record.as_ref().and(agent_tty());
                if let (Some(record), Some((_, tty))) = (&record, &tty) {
                    write(tty, &osc(record));
                }
                let what = if tty.is_some() {
                    " and its turn record"
                } else {
                    ""
                };
                log(
                    name,
                    None,
                    &format!("left the Rex pane {}{what}", status.state),
                );
            }
        }
        return;
    };
    let Some(tty) = tmux(&["display-message", "-p", "-t", &pane, "#{pane_tty}"]) else {
        return log(name, Some(&pane), "skipped: tmux gave no tty");
    };
    let pairs = pairs();
    write(&tty, &wrap(&pairs));
    if status.state == "clear" {
        tmux(&["set-option", "-p", "-u", "-t", &pane, "@rex_status"]);
    } else {
        tmux(&["set-option", "-p", "-t", &pane, "@rex_status", &pairs]);
    }
    log(name, Some(&pane), &format!("wrote {}", status.state));
}

/// `rexboard hooks replay SESSION`: re-sends each pane's last status, for a tmux client that
/// just attached to SESSION.
pub fn replay(session: &str) -> Result<()> {
    let rows = tmux(&[
        "list-panes",
        "-s",
        "-t",
        session,
        "-F",
        "#{pane_tty} #{@rex_status}",
    ]);
    for row in rows.unwrap_or_default().lines() {
        if let Some((tty, pairs)) = row.split_once(' ')
            && !pairs.is_empty()
        {
            write(tty, &wrap(pairs));
        }
    }
    Ok(())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The agent's tmux pane: TMUX_PANE, else, under a tmux TERM, the one pane on the agent's tty. A
/// launcher can start the agent without TMUX and TMUX_PANE while it still runs in a pane.
fn seat_pane() -> Option<String> {
    if let (Some(_), Some(pane)) = (env("TMUX"), env("TMUX_PANE")) {
        return Some(pane);
    }
    let term = env("TERM").unwrap_or_default();
    if !(term.starts_with("tmux") || term.starts_with("screen")) {
        return None;
    }
    let (_, tty) = agent_tty()?;
    let rows = tmux(&["list-panes", "-a", "-F", "#{pane_tty} #{pane_id}"])?;
    let panes: Vec<&str> = rows
        .lines()
        .filter_map(|row| row.strip_prefix(&format!("{tty} ")))
        .collect();
    (panes.len() == 1).then(|| panes[0].to_owned())
}

/// The agent's pid and tty: its hook processes have no tty, so walk up to the first that does.
fn agent_tty() -> Option<(u32, String)> {
    let mut pid = std::os::unix::process::parent_id();
    while pid > 1 {
        let row = output(
            &["ps", "/bin/ps"],
            &["-o", "ppid=,tty=", "-p", &pid.to_string()],
        )?;
        let words: Vec<&str> = row.split_whitespace().collect();
        let [parent, tty] = words[..] else {
            return None;
        };
        if tty != "??" {
            return Some((pid, format!("/dev/{tty}")));
        }
        pid = parent.parse().ok()?;
    }
    None
}

fn tmux(args: &[&str]) -> Option<String> {
    output(
        &["tmux", "/opt/homebrew/bin/tmux", "/usr/local/bin/tmux"],
        args,
    )
}

/// The first of `exes` that starts, its output without the final newline if it succeeds within
/// two seconds, as a hook must not hang its agent.
fn output(exes: &[&str], args: &[&str]) -> Option<String> {
    for exe in exes {
        let Ok(mut child) = Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let mut out = String::new();
        child.stdout.take()?.read_to_string(&mut out).ok()?;
        return status
            .success()
            .then(|| out.strip_suffix('\n').unwrap_or(&out).to_owned());
    }
    None
}

fn write(tty: &str, text: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(tty) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// With REXBOARD_HOOK_LOG set, one line per hook call: the event, which of TMUX, TMUX_PANE and
/// REX_BLOCK were set, the pane found, and what it did; never the hook's text.
fn log(event: Option<&str>, pane: Option<&str>, action: &str) {
    let Some(path) = env("REXBOARD_HOOK_LOG") else {
        return;
    };
    let set: Vec<&str> = ["TMUX", "TMUX_PANE", "REX_BLOCK"]
        .into_iter()
        .filter(|name| env(name).is_some())
        .collect();
    let set = if set.is_empty() {
        "-".to_owned()
    } else {
        set.join(",")
    };
    let line = format!(
        "{} {} env={set} pane={} {action}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        event.unwrap_or("-"),
        pane.unwrap_or("-"),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}
