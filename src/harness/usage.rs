//! What an agent's last turn used: its model, how full its context is, and how many tokens its
//! last request sent. The harness's own per-turn surface (Claude Code's status line, a Codex
//! hook, a pi extension) hands it to `rexboard usage`, which keeps it in a small file per pane;
//! serve reads those files and never a transcript.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A file no live pane names is removed once it is this old; younger, its pane may not be on
/// the board yet.
pub const ORPHAN_AFTER: chrono::Duration = chrono::Duration::minutes(10);

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

    /// The one-line JSON a `usage` record carries to Rex: `{"v":1,"m":"Opus","c":42,...}`, only
    /// the figures that are set, each held to its range.
    pub fn to_report(&self) -> String {
        let mut doc = serde_json::json!({ "v": REPORT_VERSION });
        let mut put = |key: &str, value: Option<serde_json::Value>| {
            if let Some(value) = value {
                doc[key] = value;
            }
        };
        put("m", self.model.as_deref().and_then(model).map(Into::into));
        put("c", self.ctx_pct.map(|p| p.min(100).into()));
        put("w", self.ctx_window.map(Into::into));
        put("t", self.tok.map(Into::into));
        put("u", self.cost_usd.and_then(cost).map(Into::into));
        put("q", self.quota_5h.map(|p| p.min(100).into()));
        doc.to_string()
    }

    /// What a `usage` record's message says; None for anything that is not this version's
    /// document, over REPORT_MAX bytes, or with no figure in it. Each figure is held to its range
    /// again, since the record came over a wire.
    pub fn from_report(text: &str) -> Option<Self> {
        if text.len() > REPORT_MAX {
            return None;
        }
        let doc: serde_json::Value = serde_json::from_str(text).ok()?;
        if doc["v"] != REPORT_VERSION {
            return None;
        }
        let usage = Self {
            model: doc["m"].as_str().and_then(model),
            ctx_pct: percent(&doc["c"]),
            ctx_window: doc["w"].as_u64(),
            tok: doc["t"].as_u64(),
            cost_usd: doc["u"].as_f64().and_then(cost),
            quota_5h: percent(&doc["q"]),
        };
        (!usage.is_empty()).then_some(usage)
    }
}

/// The id of the sub-record that carries usage to Rex, beside the agent's own status.
pub const REPORT_ID: &str = "usage";
const REPORT_VERSION: u32 = 1;
/// A record's message over this is not read.
const REPORT_MAX: usize = 512;
const MODEL_MAX: usize = 64;
/// At most one report per pane in this long, unless a turn has just ended.
pub const REPORT_EVERY: chrono::Duration = chrono::Duration::seconds(5);

/// A model name without control characters, cut to MODEL_MAX characters; None if nothing is left.
fn model(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MODEL_MAX)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// A cost in dollars to four decimals; None for a negative or non-finite one.
fn cost(usd: f64) -> Option<f64> {
    (usd.is_finite() && usd >= 0.0).then(|| (usd * 10_000.0).round() / 10_000.0)
}

/// `rexboard usage HARNESS [-- CMD]`. Keeping the numbers never breaks the status line it runs
/// in: a payload it cannot read, or a pane it cannot name, is skipped without a word.
pub fn run(name: &str, then: &[String]) -> crate::error::Result<()> {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(1 << 20).read_to_end(&mut input);
    match named(name) {
        Ok(harness) => {
            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&input) {
                keep(harness, &payload, harness.usage_per_turn());
            }
        }
        // The status line it runs in still shows: an unknown name keeps nothing, and says so.
        Err(err) => {
            eprintln!("error: {err}");
            crate::hook::note(&format!("usage: {err}"));
        }
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

/// The harness `name` names, by its id or its short name.
pub fn named(name: &str) -> crate::error::Result<&'static dyn super::Harness> {
    super::by_name(name).ok_or_else(|| {
        let names: Vec<&str> = super::BUILTIN.iter().map(|h| h.name()).collect();
        crate::error::Error::usage_with(
            format!("no harness named {name}; harnesses: {}", names.join(", ")),
            &["rexboard hooks status"],
        )
    })
}

/// Keeps what `payload` says the last turn used, for the pane this process runs in; a payload
/// with no usage, or a pane it cannot name, keeps nothing.
pub fn keep(harness: &dyn super::Harness, payload: &serde_json::Value, turn_end: bool) {
    let env = |name: &str| std::env::var(name).ok();
    if let Some(usage) = harness.usage(payload)
        && let Some(key) = own_key(
            env("TMUX").as_deref(),
            env("TMUX_PANE").as_deref(),
            env("REX_BLOCK").as_deref(),
        )
        && let Ok(paths) = crate::state::Paths::find()
    {
        let now = Utc::now();
        let last = sent_by(&paths.dir, &key);
        let report =
            env("REXBOARD_NO_USAGE_REPORT").is_none() && due(last.as_ref(), &usage, now, turn_end);
        let sent = if report {
            Some(Sent::new(&usage, now))
        } else {
            last
        };
        let _ = write(&paths.dir, &key, harness.id(), &usage, sent.clone(), now);
        if report {
            crate::hook::report_usage(harness.id(), &usage);
        }
    }
}

/// What this pane last reported to Rex, from its file.
fn sent_by(state: &Path, key: &str) -> Option<Sent> {
    let sidecar: Sidecar =
        serde_json::from_slice(&std::fs::read(dir(state).join(file_name(key))).ok()?).ok()?;
    sidecar.sent
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
        // Before the first request a session reports 0 tokens and a cost of 0: nothing yet.
        tok: ctx["total_input_tokens"].as_u64().filter(|tok| *tok > 0),
        cost_usd: p["cost"]["total_cost_usd"]
            .as_f64()
            .filter(|cost| *cost > 0.0),
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
    /// The figures last reported to Rex and when, so a report goes only on a change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sent: Option<Sent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sent {
    ts: String,
    usage: Usage,
}

impl Sent {
    pub fn new(usage: &Usage, now: DateTime<Utc>) -> Self {
        Self {
            ts: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            usage: usage.clone(),
        }
    }
}

/// Whether `usage` goes to Rex now: it is not what was last sent, and the last send was
/// REPORT_EVERY ago or more, or a turn has just ended.
pub fn due(last: Option<&Sent>, usage: &Usage, now: DateTime<Utc>, turn_end: bool) -> bool {
    let Some(last) = last else {
        return true;
    };
    if last.usage == *usage {
        return false;
    }
    let at = DateTime::parse_from_rfc3339(&last.ts).map(|at| at.with_timezone(&Utc));
    match at {
        Ok(at) => turn_end || now - at >= REPORT_EVERY,
        Err(_) => true,
    }
}

fn dir(state: &Path) -> PathBuf {
    state.join("usage")
}

/// The pane this process runs in. Outside tmux, Rex's block id. Inside tmux, REX_BLOCK is the
/// pane that started the tmux server, so the tmux socket, server and pane name it instead:
/// `tmux:<socket file name>:<server pid>:<pane id>`, which serve finds again from the tmux
/// client in front. A server restarted on the same socket numbers its panes from `%0` again;
/// its new pid keeps them apart.
pub fn own_key(
    tmux: Option<&str>,
    tmux_pane: Option<&str>,
    rex_block: Option<&str>,
) -> Option<String> {
    match tmux {
        Some(tmux) => {
            let mut parts = tmux.split(',');
            let socket = parts.next().filter(|s| !s.is_empty())?;
            let server = parts.next().filter(|s| !s.is_empty())?;
            let name = Path::new(socket).file_name()?.to_str()?;
            let pane = tmux_pane.filter(|p| !p.is_empty())?;
            Some(tmux_key(name, server, pane))
        }
        None => rex_block.filter(|b| !b.is_empty()).map(str::to_owned),
    }
}

pub fn tmux_key(socket: &str, server: &str, pane: &str) -> String {
    format!("tmux:{socket}:{server}:{pane}")
}

/// Whether any file in `usage/` is a tmux pane's, so serve asks tmux only when one is.
pub fn any_tmux(state: &Path) -> bool {
    std::fs::read_dir(dir(state))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("tmux-"))
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
    sent: Option<Sent>,
    now: DateTime<Utc>,
) -> std::io::Result<()> {
    let dir = dir(state);
    std::fs::create_dir_all(&dir)?;
    let sidecar = Sidecar {
        key: key.to_owned(),
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        harness: harness.to_owned(),
        usage: usage.clone(),
        sent,
    };
    let path = dir.join(file_name(key));
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(
        &tmp,
        serde_json::to_vec(&sidecar).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

/// What one file held: its pane's key and the usage.
type Kept = Option<(String, Usage)>;

/// serve's view of `usage/`: each file's modified time and what it held, so a scan reads only
/// the files that changed since the last one.
#[derive(Default)]
pub struct Files {
    seen: HashMap<PathBuf, (SystemTime, Kept)>,
}

impl Files {
    /// Every file's usage, by its key, at any age: the last turn's figures stay true while a
    /// pane is idle. With `live`, the keys of the panes on the board, a file no live pane names
    /// is removed once it is ORPHAN_AFTER old, and a stray temp file too; without it (before
    /// the first snapshot) nothing is removed. A file that cannot be read is skipped until it
    /// changes.
    pub fn read(
        &mut self,
        state: &Path,
        live: Option<&BTreeSet<String>>,
        now: DateTime<Utc>,
    ) -> BTreeMap<String, Usage> {
        let mut seen = HashMap::new();
        let mut kept = BTreeMap::new();
        for entry in std::fs::read_dir(dir(state))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let json = path.extension().is_some_and(|ext| ext == "json");
            let held = match self.seen.remove(&path) {
                Some((at, held)) if at == modified => held,
                _ if json => read_one(&path),
                _ => None,
            };
            let orphan = live.is_some_and(|live| {
                held.as_ref().is_none_or(|(key, _)| !live.contains(key))
                    && now.signed_duration_since(DateTime::<Utc>::from(modified)) > ORPHAN_AFTER
            });
            if orphan {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if let Some((key, usage)) = &held {
                kept.insert(key.clone(), usage.clone());
            }
            seen.insert(path, (modified, held));
        }
        self.seen = seen;
        kept
    }
}

fn read_one(path: &Path) -> Kept {
    let sidecar: Sidecar = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some((sidecar.key, sidecar.usage))
}

/// What the tmux servers behind some clients say: each client's pid with the
/// `tmux:<socket>:<server>:<pane>` key of the pane it shows, what a bridged row is joined to its
/// usage by; and the key of every pane on those servers, shown or not, which are all live.
#[derive(Debug, Default)]
pub struct Tmux {
    pub shown: BTreeMap<u32, String>,
    pub panes: BTreeSet<String>,
}

/// Every process's parent and command, from one `ps`, for finding a pane's tmux client when the
/// pane runs tmux under a wrapper.
#[derive(Debug, Default)]
pub struct Procs(Vec<(u32, u32, String)>);

impl Procs {
    pub fn read() -> Self {
        let out = Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        out.map(|out| Self::parse(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default()
    }

    pub(crate) fn parse(out: &str) -> Self {
        Self(
            out.lines()
                .filter_map(|line| {
                    let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
                    let (ppid, comm) = rest.trim_start().split_once(char::is_whitespace)?;
                    Some((
                        pid.parse().ok()?,
                        ppid.parse().ok()?,
                        comm.trim().to_owned(),
                    ))
                })
                .collect(),
        )
    }

    /// The tmux client that is the one tmux child of `parent`: a pane runs one under OpenRig's
    /// `sh -c ... tmux attach` wrapper, which is what Rex reports as the foreground process. With
    /// two, which one the pane shows is a guess, so there is none.
    pub fn tmux_child(&self, parent: u32) -> Option<u32> {
        let mut found = self
            .0
            .iter()
            .filter(|(_, ppid, comm)| *ppid == parent && comm.rsplit('/').next() == Some("tmux"))
            .map(|(pid, ..)| *pid);
        found.next().filter(|_| found.next().is_none())
    }
}

/// One `ps` per client, and `tmux list-clients` and `list-panes -a` per socket; a client whose
/// socket or pane cannot be read is left out.
pub fn tmux_keys(pids: &[u32]) -> Tmux {
    let mut sockets: BTreeMap<PathBuf, Vec<u32>> = BTreeMap::new();
    for &pid in pids {
        if let Some(socket) = client_socket(pid) {
            sockets.entry(socket).or_default().push(pid);
        }
    }
    let mut tmux = Tmux::default();
    for (socket, wanted) in sockets {
        let Some(name) = socket.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let ask = |args: &[&str]| {
            Command::new("tmux")
                .arg("-S")
                .arg(&socket)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                .unwrap_or_default()
        };
        let listed = ask(&["list-clients", "-F", "#{client_pid} #{pid} #{pane_id}"]);
        tmux.shown.extend(clients(&listed, name, &wanted));
        let listed = ask(&["list-panes", "-a", "-F", "#{pid} #{pane_id}"]);
        tmux.panes.extend(panes(&listed, name));
    }
    tmux
}

/// `list-panes -a` lines (`server_pid pane_id`) as pane keys.
fn panes(out: &str, socket: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let (server, pane) = line.split_once(' ')?;
            Some(tmux_key(socket, server, pane))
        })
        .collect()
}

/// `list-clients` lines (`client_pid server_pid pane_id`) as each wanted client's pane key.
fn clients(out: &str, socket: &str, wanted: &[u32]) -> Vec<(u32, String)> {
    out.lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            let pid: u32 = words.next()?.parse().ok()?;
            let (server, pane) = (words.next()?, words.next()?);
            wanted
                .contains(&pid)
                .then(|| (pid, tmux_key(socket, server, pane)))
        })
        .collect()
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
    fn a_wrappers_tmux_child_is_its_client() {
        let procs = Procs::parse(
            "    1     0 /sbin/launchd\n  \
             200     1 /usr/local/bin/tmux\n  \
             310   100 /bin/sh\n  \
             311   310 /usr/local/bin/tmux\n  \
             320   100 /bin/sh\n  \
             321   320 /usr/bin/tail\n  \
             330   100 /Apps/My Tool/tmux\n  \
             410   400 /bin/sh\n  \
             411   410 /usr/local/bin/tmux\n  \
             412   410 /usr/local/bin/tmux\n  \
             garbage\n",
        );
        assert_eq!(procs.tmux_child(310), Some(311), "tmux under its wrapper");
        assert_eq!(procs.tmux_child(320), None, "a wrapper of something else");
        assert_eq!(procs.tmux_child(311), None, "a client has no client");
        assert_eq!(procs.tmux_child(100), Some(330), "a path with a space");
        assert_eq!(
            procs.tmux_child(410),
            None,
            "two tmux children: which is shown is a guess"
        );
        assert_eq!(Procs::default().tmux_child(1), None);
    }

    fn full() -> Usage {
        Usage {
            model: Some("Opus 5.5".into()),
            ctx_pct: Some(42),
            ctx_window: Some(200_000),
            tok: Some(84_210),
            cost_usd: Some(1.2345),
            quota_5h: Some(17),
        }
    }

    #[test]
    fn a_report_round_trips_and_leaves_out_what_is_not_set() {
        let report = full().to_report();
        assert_eq!(
            report,
            r#"{"v":1,"m":"Opus 5.5","c":42,"w":200000,"t":84210,"u":1.2345,"q":17}"#
        );
        assert!(report.len() < 120);
        assert_eq!(Usage::from_report(&report), Some(full()));
        let part = Usage {
            model: Some("grok-build-1".into()),
            ctx_pct: Some(12),
            ..Usage::default()
        };
        assert_eq!(part.to_report(), r#"{"v":1,"m":"grok-build-1","c":12}"#);
        assert_eq!(Usage::from_report(&part.to_report()), Some(part));
    }

    #[test]
    fn a_report_holds_every_figure_to_its_range_on_both_ends() {
        let long = Usage {
            model: Some(format!("a\u{7}b\n{}", "m".repeat(100))),
            ctx_pct: Some(250),
            quota_5h: Some(101),
            cost_usd: Some(0.123_456_789),
            ..Usage::default()
        };
        let back = Usage::from_report(&long.to_report()).unwrap();
        assert_eq!(back.model.as_deref().map(|m| m.chars().count()), Some(64));
        assert!(!back.model.unwrap().contains(['\u{7}', '\n']));
        assert_eq!(
            (back.ctx_pct, back.quota_5h, back.cost_usd),
            (Some(100), Some(100), Some(0.1235))
        );
        // A document that did not come from to_report is held to the same ranges.
        let wild = Usage::from_report(r#"{"v":1,"m":"\u0007x","c":250,"u":-3,"q":-5}"#).unwrap();
        assert_eq!(
            (
                wild.model.as_deref(),
                wild.ctx_pct,
                wild.cost_usd,
                wild.quota_5h
            ),
            (Some("x"), Some(100), None, Some(0))
        );
        assert_eq!(
            Usage {
                cost_usd: Some(f64::NAN),
                ..Usage::default()
            }
            .to_report(),
            r#"{"v":1}"#
        );
    }

    #[test]
    fn a_report_that_is_not_this_versions_or_is_too_big_is_not_read() {
        for text in [
            "",
            "{",
            "[]",
            r#"{"c":1}"#,
            r#"{"v":2,"c":1}"#,
            r#"{"v":1}"#,
            r#"{"v":1,"m":"  "}"#,
        ] {
            assert_eq!(Usage::from_report(text), None, "{text:?}");
        }
        let big = format!(r#"{{"v":1,"m":"x","pad":"{}"}}"#, "p".repeat(REPORT_MAX));
        assert_eq!(Usage::from_report(&big), None, "over {REPORT_MAX} bytes");
    }

    #[test]
    fn a_report_is_due_on_a_change_after_5_seconds_or_at_a_turn_end() {
        let now = Utc::now();
        let sent = |ago: i64, usage: &Usage| Sent::new(usage, now - chrono::Duration::seconds(ago));
        let (a, b) = (
            full(),
            Usage {
                ctx_pct: Some(43),
                ..full()
            },
        );
        assert!(due(None, &a, now, false), "never sent");
        assert!(
            !due(Some(&sent(600, &a)), &a, now, true),
            "the same figures, even at a turn end"
        );
        assert!(
            !due(Some(&sent(4, &a)), &b, now, false),
            "a change inside 5 s waits"
        );
        assert!(
            due(Some(&sent(5, &a)), &b, now, false),
            "a change after 5 s goes"
        );
        assert!(
            due(Some(&sent(1, &a)), &b, now, true),
            "a turn end does not wait"
        );
        let mut broken = sent(0, &a);
        broken.ts = "later".into();
        assert!(
            due(Some(&broken), &b, now, false),
            "an unreadable time does not hold a report"
        );
    }

    #[test]
    fn what_was_last_sent_survives_each_write_and_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let mark = Sent::new(&full(), now);
        write(
            dir.path(),
            "block:x",
            "claude-code",
            &Usage {
                ctx_pct: Some(43),
                ..full()
            },
            Some(mark),
            now,
        )
        .unwrap();
        let sent = sent_by(dir.path(), "block:x").unwrap();
        assert_eq!(
            sent.usage.ctx_pct,
            Some(42),
            "the file keeps what Rex was told, not the latest"
        );
        assert!(sent_by(dir.path(), "block:none").is_none());
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
        assert_eq!(
            parse("claude", &fixture("claude-statusline-start.json")),
            Some(Usage {
                model: Some("Opus 5.5".into()),
                ctx_window: Some(1_000_000),
                ..Usage::default()
            }),
            "before the first request: 0 tokens and a cost of 0 are blank, not 0"
        );
    }

    #[test]
    fn pi_reads_its_extensions_turn_end_and_a_zero_cost_is_blank() {
        assert_eq!(
            parse("pi", &fixture("pi-turn-end.json")),
            Some(Usage {
                model: Some("claude-sonnet-5-5".into()),
                ctx_pct: Some(14),
                ctx_window: Some(200_000),
                // input 25927 + cacheRead 1152 + cacheWrite 0; not the context's 27111, which
                // counts the reply's 32 too.
                tok: Some(27079),
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
    fn a_pane_is_keyed_by_its_block_outside_tmux_and_by_its_tmux_server_and_pane_inside() {
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
            Some("tmux:default:1234:%3"),
            "inside tmux, REX_BLOCK is the server's pane, not this one"
        );
        assert_ne!(
            own_key(Some("/tmp/tmux-501/default,5678,0"), Some("%3"), None),
            own_key(Some("/tmp/tmux-501/default,1234,0"), Some("%3"), None),
            "a restarted server's %3 is another pane"
        );
        assert_eq!(
            own_key(Some("/tmp/tmux-501/default,1,0"), None, Some("block:x")),
            None
        );
        assert_eq!(
            own_key(Some("/tmp/tmux-501/default"), Some("%3"), None),
            None
        );
        assert_eq!(own_key(None, None, Some("")), None);
    }

    #[test]
    fn a_tmux_client_is_joined_to_its_server_and_pane() {
        let out = "4242 1234 %3\n5151 1234 %0\nbad line\n";
        assert_eq!(
            clients(out, "default", &[4242]),
            [(4242, "tmux:default:1234:%3".to_owned())],
            "the key the pane's own `rexboard usage` wrote"
        );
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
    fn a_tmux_pane_no_client_shows_still_keeps_its_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let usage = Usage {
            ctx_pct: Some(41),
            ..Usage::default()
        };
        // The client shows %3; the agent in %5 sits in a window nobody looks at.
        write(
            dir.path(),
            "tmux:seats:1234:%5",
            "claude-code",
            &usage,
            None,
            now,
        )
        .unwrap();
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("usage/tmux-seats-1234--5.json"))
            .unwrap()
            .set_modified(SystemTime::from(now - ORPHAN_AFTER * 3))
            .unwrap();
        let shown = clients("4242 1234 %3\n", "seats", &[4242]);
        let live: BTreeSet<String> = shown
            .into_iter()
            .map(|(_, key)| key)
            .chain(panes("1234 %3\n1234 %5\n", "seats"))
            .collect();
        let read = Files::default().read(dir.path(), Some(&live), now);
        assert_eq!(
            read["tmux:seats:1234:%5"], usage,
            "every pane on the server is live"
        );
    }

    #[test]
    fn written_usage_reads_back_at_any_age_while_its_pane_lives() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let usage = Usage {
            model: Some("Opus 5.5".into()),
            ctx_pct: Some(7),
            tok: Some(67910),
            ..Usage::default()
        };
        write(
            dir.path(),
            "tmux:default:1234:%3",
            "claude-code",
            &usage,
            None,
            now,
        )
        .unwrap();
        std::fs::write(dir.path().join("usage/broken.json"), "{").unwrap();
        let mut files = Files::default();
        let read = files.read(dir.path(), None, now);
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read["tmux:default:1234:%3"], usage);
        let live = BTreeSet::from(["tmux:default:1234:%3".to_owned()]);
        let later = now + chrono::Duration::minutes(30);
        assert_eq!(
            files.read(dir.path(), Some(&live), later),
            read,
            "30 minutes idle, its last turn still stands"
        );
        let file =
            std::fs::read_to_string(dir.path().join("usage/tmux-default-1234--3.json")).unwrap();
        assert!(
            !file.contains("cost_usd"),
            "a figure not reported is left out: {file}"
        );
    }

    #[test]
    fn a_scan_rereads_only_a_changed_file_and_removes_one_no_live_pane_names() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let usage = |pct| Usage {
            ctx_pct: Some(pct),
            ..Usage::default()
        };
        write(
            dir.path(),
            "block:live",
            "claude-code",
            &usage(7),
            None,
            now,
        )
        .unwrap();
        write(
            dir.path(),
            "block:gone",
            "claude-code",
            &usage(9),
            None,
            now,
        )
        .unwrap();
        write(dir.path(), "block:new", "claude-code", &usage(3), None, now).unwrap();
        let usage_dir = dir.path().join("usage");
        std::fs::write(usage_dir.join("left.json.4242"), "{").unwrap();
        let age = |name: &str, by: chrono::Duration| {
            let at = SystemTime::from(now - by);
            std::fs::File::options()
                .write(true)
                .open(usage_dir.join(name))
                .unwrap()
                .set_modified(at)
                .unwrap();
        };
        let live = BTreeSet::from(["block:live".to_owned()]);
        let mut files = Files::default();
        assert_eq!(files.read(dir.path(), Some(&live), now).len(), 3);
        // Same modified time, new bytes: the scan keeps what it read.
        let path = usage_dir.join("block-live.json");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap().replace("7", "8"),
        )
        .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(
            files.read(dir.path(), Some(&live), now)["block:live"],
            usage(7)
        );
        write(
            dir.path(),
            "block:live",
            "claude-code",
            &usage(12),
            None,
            now,
        )
        .unwrap();
        age("block-live.json", chrono::Duration::seconds(-1));
        assert_eq!(
            files.read(dir.path(), Some(&live), now)["block:live"],
            usage(12),
            "a rewrite is read"
        );
        for name in ["block-live.json", "block-gone.json", "left.json.4242"] {
            age(name, ORPHAN_AFTER * 2);
        }
        let left = |files: &mut Files, live: Option<&BTreeSet<String>>| {
            files.read(dir.path(), live, now);
            let mut names: Vec<_> = std::fs::read_dir(&usage_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        assert_eq!(
            left(&mut files, None).len(),
            4,
            "before the first snapshot nothing is removed"
        );
        assert_eq!(
            left(&mut files, Some(&live)),
            ["block-live.json", "block-new.json"],
            "a closed pane's old file and a stray temp file go; a live pane's stays at any age, \
             and a young one may be a pane the board has not seen yet"
        );
    }
}
