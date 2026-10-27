//! What an agent's last turn used: its model, how full its context is, and how many tokens its
//! last request sent. The harness's own per-turn surface (Claude Code's status line, a Codex
//! hook, a pi extension) hands it to `rexboard usage`, which keeps it in a small file per pane;
//! serve reads those files and never a transcript.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A file older than this says nothing about the pane now.
pub const STALE_AFTER: chrono::Duration = chrono::Duration::minutes(10);

/// Blank is blank: a harness that does not report a figure leaves it out, never zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Percent of the context window in use, 0-100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_window: Option<u64>,
    /// The prompt side of the last model request, cache hits included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tok: Option<u64>,
    /// The harness's own estimate, at list price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Percent of the five-hour quota used, where the plan has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_5h: Option<u8>,
}

impl Usage {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `rexboard usage HARNESS [-- CMD]`. Keeping the numbers never breaks the status line it runs
/// in: a payload it cannot read, or a pane it cannot name, is skipped without a word.
pub fn run(name: &str, then: &[String]) -> crate::error::Result<()> {
    let harness = super::by_name(name).ok_or_else(|| {
        let names: Vec<&str> = super::BUILTIN.iter().map(|h| h.name()).collect();
        crate::error::Error::usage_with(
            format!("no harness named {name}; harnesses: {}", names.join(", ")),
            &["rexboard usage claude -- your-status-line"],
        )
    })?;
    let mut input = Vec::new();
    std::io::stdin().take(1 << 20).read_to_end(&mut input)?;
    let env = |name: &str| std::env::var(name).ok();
    if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&input)
        && let Some(usage) = harness.usage(&payload)
        && let Some(key) = own_key(
            env("TMUX").as_deref(),
            env("TMUX_PANE").as_deref(),
            env("REX_BLOCK").as_deref(),
        )
        && let Ok(paths) = crate::state::Paths::find()
    {
        let _ = write(&paths.dir, &key, harness.id(), &usage, Utc::now());
    }
    let Some((cmd, args)) = then.split_first() else {
        return Ok(());
    };
    let mut child = Command::new(cmd).args(args).stdin(Stdio::piped()).spawn()?;
    // A command that does not read its stdin closes it early; that is its business.
    let _ = child.stdin.take().expect("piped").write_all(&input);
    let status = child.wait()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// A percent as Rex and the harnesses report it, rounded into 0-100.
pub fn percent(value: &serde_json::Value) -> Option<u8> {
    value.as_f64().map(|p| p.round().clamp(0.0, 100.0) as u8)
}

/// The status line JSON Claude Code (and grok, in the same shape) pipes to its status line
/// command on each new message.
pub fn status_line(p: &serde_json::Value) -> Option<Usage> {
    let ctx = &p["context_window"];
    let model = &p["model"];
    let usage = Usage {
        model: model["display_name"]
            .as_str()
            .or_else(|| model["id"].as_str())
            .map(str::to_owned),
        ctx_pct: percent(&ctx["used_percentage"]),
        ctx_window: ctx["context_window_size"].as_u64(),
        tok: ctx["total_input_tokens"].as_u64(),
        cost_usd: p["cost"]["total_cost_usd"].as_f64(),
        quota_5h: percent(&p["rate_limits"]["five_hour"]["used_percentage"]),
    };
    (!usage.is_empty()).then_some(usage)
}

/// One pane's file: which pane, when, from which harness, and what it used.
#[derive(Debug, Serialize, Deserialize)]
struct Sidecar {
    key: String,
    ts: String,
    harness: String,
    #[serde(flatten)]
    usage: Usage,
}

fn dir(state: &Path) -> PathBuf {
    state.join("usage")
}

/// The pane this process runs in. Outside tmux, Rex's block id. Inside tmux, REX_BLOCK is the
/// pane that started the tmux server, so the tmux socket and pane name it instead:
/// `tmux:<socket file name>:<pane id>`, which serve finds again from the tmux client in front.
pub fn own_key(
    tmux: Option<&str>,
    tmux_pane: Option<&str>,
    rex_block: Option<&str>,
) -> Option<String> {
    match tmux {
        Some(tmux) => {
            let socket = tmux.split(',').next().filter(|s| !s.is_empty())?;
            let name = Path::new(socket).file_name()?.to_str()?;
            let pane = tmux_pane.filter(|p| !p.is_empty())?;
            Some(tmux_key(name, pane))
        }
        None => rex_block.filter(|b| !b.is_empty()).map(str::to_owned),
    }
}

pub fn tmux_key(socket: &str, pane: &str) -> String {
    format!("tmux:{socket}:{pane}")
}

/// A key as a file name: only letters, digits, `.`, `_` and `-`.
fn file_name(key: &str) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{safe}.json")
}

/// Writes the file whole, through a rename, so serve never reads half of one.
pub fn write(
    state: &Path,
    key: &str,
    harness: &str,
    usage: &Usage,
    now: DateTime<Utc>,
) -> std::io::Result<()> {
    let dir = dir(state);
    std::fs::create_dir_all(&dir)?;
    let sidecar = Sidecar {
        key: key.to_owned(),
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        harness: harness.to_owned(),
        usage: usage.clone(),
    };
    let path = dir.join(file_name(key));
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(
        &tmp,
        serde_json::to_vec(&sidecar).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

/// Every pane's usage that is not stale at `now`, by its key. A file that cannot be read is
/// skipped: a writer may be halfway through, and the next scan sees it whole.
pub fn read_all(state: &Path, now: DateTime<Utc>) -> BTreeMap<String, Usage> {
    let Ok(entries) = std::fs::read_dir(dir(state)) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let sidecar: Sidecar = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            let ts = DateTime::parse_from_rfc3339(&sidecar.ts).ok()?;
            (now.signed_duration_since(ts) <= STALE_AFTER).then_some((sidecar.key, sidecar.usage))
        })
        .collect()
}

/// Each tmux client's pid with the `tmux:<socket>:<pane>` key of the pane it shows: what a
/// bridged row is joined to its usage by. One `ps` per client and one `tmux list-clients` per
/// socket; a client whose socket or pane cannot be read is left out.
pub fn tmux_keys(pids: &[u32]) -> BTreeMap<u32, String> {
    let mut sockets: BTreeMap<PathBuf, Vec<u32>> = BTreeMap::new();
    for &pid in pids {
        if let Some(socket) = client_socket(pid) {
            sockets.entry(socket).or_default().push(pid);
        }
    }
    let mut keys = BTreeMap::new();
    for (socket, wanted) in sockets {
        let Some(name) = socket.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(out) = Command::new("tmux")
            .arg("-S")
            .arg(&socket)
            .args(["list-clients", "-F", "#{client_pid} #{pane_id}"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((pid, pane)) = line.split_once(' ')
                && let Ok(pid) = pid.parse()
                && wanted.contains(&pid)
            {
                keys.insert(pid, tmux_key(name, pane));
            }
        }
    }
    keys
}

/// The socket a tmux client talks to, from its command line: `-S path`, `-L name`, or the
/// default one.
fn client_socket(pid: u32) -> Option<PathBuf> {
    let out = Command::new("ps")
        .args(["-o", "args=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    socket_of(&String::from_utf8_lossy(&out.stdout))
}

fn socket_of(args: &str) -> Option<PathBuf> {
    let words: Vec<&str> = args.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let dir = || {
        let base = std::env::var_os("TMUX_TMPDIR").unwrap_or_else(|| "/tmp".into());
        PathBuf::from(base).join(format!("tmux-{}", unsafe { libc::getuid() }))
    };
    for (i, word) in words.iter().enumerate() {
        let next = || words.get(i + 1).map(|w| (*w).to_owned());
        match *word {
            "-S" => return next().map(PathBuf::from),
            "-L" => return next().map(|name| dir().join(name)),
            w if w.starts_with("-S") && w.len() > 2 => return Some(PathBuf::from(&w[2..])),
            w if w.starts_with("-L") && w.len() > 2 => return Some(dir().join(&w[2..])),
            _ => {}
        }
    }
    Some(dir().join("default"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/tests/fixtures/harness/usage/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn parse(harness: &str, payload: &serde_json::Value) -> Option<Usage> {
        crate::harness::by_name(harness).unwrap().usage(payload)
    }

    #[test]
    fn claude_and_grok_read_their_status_line() {
        assert_eq!(
            parse("claude", &fixture("claude-statusline.json")),
            Some(Usage {
                model: Some("Opus 5.5".into()),
                ctx_pct: Some(7),
                ctx_window: Some(1_000_000),
                tok: Some(67910),
                cost_usd: Some(0.543476),
                quota_5h: Some(5),
            })
        );
        assert_eq!(
            parse("grok", &fixture("grok-statusline.json")),
            Some(Usage {
                model: Some("grok-build-1".into()),
                ctx_pct: Some(12),
                ..Usage::default()
            }),
            "what grok does not report stays blank"
        );
        assert_eq!(parse("claude", &serde_json::json!({ "cwd": "/x" })), None);
    }

    #[test]
    fn pi_reads_its_extensions_turn_end_and_a_zero_cost_is_blank() {
        assert_eq!(
            parse("pi", &fixture("pi-turn-end.json")),
            Some(Usage {
                model: Some("claude-sonnet-5-5".into()),
                ctx_pct: Some(23),
                ctx_window: Some(200_000),
                tok: Some(45210),
                cost_usd: None,
                quota_5h: None,
            })
        );
    }

    #[test]
    fn codex_reads_the_last_token_count_in_its_rollouts_end_as_its_footer_does() {
        let rollout = format!(
            "{}/tests/fixtures/harness/usage/codex-rollout.jsonl",
            env!("CARGO_MANIFEST_DIR")
        );
        let stop = serde_json::json!({
            "hook_event_name": "Stop", "model": "gpt-5.5-codex", "transcript_path": rollout,
        });
        assert_eq!(
            parse("codex", &stop),
            Some(Usage {
                model: Some("gpt-5.5-codex".into()),
                // (32495 - 12000) / (475000 - 12000) = 4.43%, as the footer's `Context 4% used`.
                ctx_pct: Some(4),
                ctx_window: Some(475_000),
                tok: Some(32490),
                cost_usd: None,
                quota_5h: Some(12),
            })
        );
        let gone =
            serde_json::json!({ "model": "gpt-5.5-codex", "transcript_path": "/nonexistent" });
        assert_eq!(
            parse("codex", &gone),
            Some(Usage {
                model: Some("gpt-5.5-codex".into()),
                ..Usage::default()
            }),
            "no rollout: the model alone"
        );
    }

    #[test]
    fn a_pane_is_keyed_by_its_block_outside_tmux_and_by_its_tmux_pane_inside() {
        assert_eq!(
            own_key(None, None, Some("block:01a1")).as_deref(),
            Some("block:01a1")
        );
        assert_eq!(
            own_key(
                Some("/tmp/tmux-501/default,1234,0"),
                Some("%3"),
                Some("block:stale")
            )
            .as_deref(),
            Some("tmux:default:%3"),
            "inside tmux, REX_BLOCK is the server's pane, not this one"
        );
        assert_eq!(
            own_key(Some("/tmp/tmux-501/default,1,0"), None, Some("block:x")),
            None
        );
        assert_eq!(own_key(None, None, Some("")), None);
    }

    #[test]
    fn a_tmux_clients_socket_comes_from_its_command_line() {
        let named = |args: &str| socket_of(args).map(|p| p.file_name().unwrap().to_owned());
        assert_eq!(named("tmux attach -t seat").unwrap(), "default");
        assert_eq!(
            named("/opt/homebrew/bin/tmux -L sysmon attach -t sysmon").unwrap(),
            "sysmon"
        );
        assert_eq!(named("tmux -Lseats attach").unwrap(), "seats");
        assert_eq!(
            socket_of("tmux -S /var/run/s attach"),
            Some(PathBuf::from("/var/run/s"))
        );
        assert_eq!(socket_of(""), None, "the client is gone");
    }

    #[test]
    fn written_usage_reads_back_until_it_goes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let usage = Usage {
            model: Some("Opus 5.5".into()),
            ctx_pct: Some(7),
            tok: Some(67910),
            ..Usage::default()
        };
        write(dir.path(), "tmux:default:%3", "claude-code", &usage, now).unwrap();
        write(
            dir.path(),
            "block:01a1",
            "codex",
            &Usage::default(),
            now - STALE_AFTER * 2,
        )
        .unwrap();
        std::fs::write(dir.path().join("usage/broken.json"), "{").unwrap();
        let read = read_all(dir.path(), now);
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read["tmux:default:%3"], usage);
        let file = std::fs::read_to_string(dir.path().join("usage/tmux-default--3.json")).unwrap();
        assert!(
            !file.contains("cost_usd"),
            "a figure not reported is left out: {file}"
        );
    }
}
