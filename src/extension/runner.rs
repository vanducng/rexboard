//! Runs one enabled extension on its own thread: every `every` seconds it sends the current rows
//! to the adapter and reports what came back, so a slow adapter never holds up serve.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::Extension;
use super::contract::{self, Item, MAX_OUTPUT, Parsed, Request, RowIn};

/// A failing extension keeps its last good result for this many runs, then loses it.
pub const KEEP_FOR: u32 = 3;
/// The most of an adapter's stderr that reaches serve.log per run.
const STDERR_KEEP: u64 = 4096;

/// How the last run went, as serve reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunState {
    pub ok: bool,
    pub error: Option<String>,
    pub at: String,
    pub ms: u64,
}

/// One run's outcome. `results` replaces the extension's rows when set; an empty map drops them.
#[derive(Debug)]
pub struct Report {
    pub name: String,
    pub state: RunState,
    pub results: Option<BTreeMap<String, Item>>,
}

/// The rows serve last saw, which each run sends.
pub type Rows = Arc<Mutex<Vec<RowIn>>>;

/// A running extension; dropping it stops the thread at its next check.
pub struct Runner {
    pub ext: Extension,
    pub settings: Value,
    stop: Arc<AtomicBool>,
}

impl Runner {
    pub fn spawn(ext: Extension, settings: Value, rows: Rows, tx: Sender<Report>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (e, s, flag) = (ext.clone(), settings.clone(), stop.clone());
        thread::spawn(move || run_loop(&e, &s, &rows, &tx, &flag));
        Self {
            ext,
            settings,
            stop,
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn run_loop(
    ext: &Extension,
    settings: &Value,
    rows: &Rows,
    tx: &Sender<Report>,
    stop: &AtomicBool,
) {
    let name = ext.name();
    let every = Duration::from_secs(ext.manifest.every);
    let stopped = || stop.load(Ordering::SeqCst);
    let mut failures = 0;
    let mut said: BTreeSet<String> = BTreeSet::new();
    let mut next = Instant::now();
    while !stopped() {
        let now = Instant::now();
        if now < next {
            thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        let snapshot = rows.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // Serve has no rows until a host reports, and an adapter only speaks of rows.
        if snapshot.is_empty() {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let result = run_once(ext, settings, &snapshot, &stopped);
        if stopped() {
            return;
        }
        let ms = u64::try_from(now.elapsed().as_millis()).unwrap_or(u64::MAX);
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let (error, results) = match result {
            Ok(parsed) => {
                failures = 0;
                // Each problem once, so an adapter that keeps sending it does not flood the log.
                for problem in parsed.problems {
                    if said.insert(problem.clone()) {
                        eprintln!("ext {name}: {problem}");
                    }
                }
                (None, Some(parsed.rows))
            }
            Err(why) => {
                failures += 1;
                if said.insert(why.clone()) {
                    eprintln!("ext {name}: {why}");
                }
                (Some(why), (failures == KEEP_FOR).then(BTreeMap::new))
            }
        };
        let state = RunState {
            ok: error.is_none(),
            error,
            at,
            ms,
        };
        let report = Report {
            name: name.to_owned(),
            state,
            results,
        };
        if tx.send(report).is_err() {
            return;
        }
        // A run that outlasts its interval skips the ticks it missed.
        next = now + every;
        while next <= Instant::now() {
            next += every;
        }
    }
}

/// How long a finished run waits for the adapter's stderr to close.
const STDERR_GRACE: Duration = Duration::from_millis(500);

/// Runs the adapter once on `rows`, within the manifest's timeout, and reads its answer. What it
/// said on stderr goes to ours, each line as `ext NAME: LINE`.
pub fn run_once(
    ext: &Extension,
    settings: &Value,
    rows: &[RowIn],
    stopped: &dyn Fn() -> bool,
) -> Result<Parsed, String> {
    run_once_with(ext, settings, rows, stopped, &|line| {
        eprintln!("ext {}: {line}", ext.name());
    })
}

/// `run_once`, giving each line of the adapter's stderr to `emit` before it returns, so a command
/// that ends with the run (`ext test`) has not lost them.
pub fn run_once_with(
    ext: &Extension,
    settings: &Value,
    rows: &[RowIn],
    stopped: &dyn Fn() -> bool,
    emit: &dyn Fn(&str),
) -> Result<Parsed, String> {
    let name = ext.name().to_owned();
    let request = Request {
        contract: super::CONTRACT,
        extension: &name,
        settings,
        rows,
    };
    let input = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    let argv = ext.argv();
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(&ext.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own group, so a timeout kills whatever the adapter started too.
        .process_group(0);
    let mut child =
        crate::rex::spawn(&mut cmd).map_err(|err| format!("could not run {}: {err}", argv[0]))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // Never joined: an adapter that leaves its stdin unread must not hold the run.
    thread::spawn(move || stdin.write_all(&input));
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout
            .by_ref()
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut out);
        let _ = out_tx.send(out);
    });
    let stderr = child.stderr.take().expect("piped stderr");
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = err_tx.send(stderr_lines(stderr));
    });
    let limit = Duration::from_secs(ext.manifest.timeout);
    let deadline = Instant::now() + limit;
    // Said here, on this thread: a reader thread that prints on its own can lose the race with a
    // process that exits when the run does. Once the adapter has exited and its group is killed
    // its stderr is closed, so the lines come at once; only something that left the group
    // (setsid) can hold it open, and that is given STDERR_GRACE before the lines are let go.
    let say = || {
        for line in err_rx.recv_timeout(STDERR_GRACE).unwrap_or_default() {
            emit(&line);
        }
    };
    let pid = child.id() as i32;
    let status = loop {
        if exited(pid) {
            // Whatever the adapter left running would hold its pipes open past the run.
            kill_group(pid);
            break child.wait().map_err(|err| err.to_string())?;
        }
        let late = Instant::now() >= deadline;
        if late || stopped() {
            kill_group(pid);
            let _ = child.wait();
            say();
            return Err(if late {
                format!("timed out after {}s and was stopped", limit.as_secs())
            } else {
                "stopped".to_owned()
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    say();
    // A process that left the group (setsid) can still hold stdout open; the run's deadline
    // bounds the wait for it, and the reader thread is left behind.
    let out = out_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "its output stayed open after it exited".to_owned())?;
    if !status.success() {
        return Err(format!("exited with {status}"));
    }
    let keys = rows.iter().map(|row| row.key.clone()).collect();
    contract::parse(&out, ext, &keys)
}

/// Whether the adapter has exited, without reaping it: until it is reaped its pid still names
/// its process group, so the group can be killed without reaching a process that reused the pid.
pub(super) fn exited(pid: i32) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    let rc = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &raw mut info, flags) };
    rc == 0 && unsafe { info.si_pid() } != 0
}

pub(super) fn kill_group(pid: i32) {
    unsafe { libc::kill(-pid, libc::SIGKILL) };
}

/// The first STDERR_KEEP bytes of an adapter's stderr as lines, and how much more was dropped.
/// The rest is still read, so the adapter never blocks on a full pipe.
fn stderr_lines(mut stderr: impl Read) -> Vec<String> {
    let mut kept = Vec::new();
    let _ = stderr.by_ref().take(STDERR_KEEP).read_to_end(&mut kept);
    let dropped = std::io::copy(&mut stderr, &mut std::io::sink()).unwrap_or(0);
    let mut lines: Vec<String> = String::from_utf8_lossy(&kept)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    if dropped > 0 {
        lines.push(format!("{dropped} more bytes of stderr dropped"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::tests::{manifest, write};

    /// An extension whose adapter is `script`, a POSIX shell body.
    fn adapter(root: &std::path::Path, script: &str, extra: &str) -> Extension {
        use std::os::unix::fs::PermissionsExt;
        // Top-level keys go before the field tables.
        let text =
            manifest("fake", &["rig"]).replacen("\n[[fields]]", &format!("\n{extra}[[fields]]"), 1);
        let dir = write(root, "fake", &text);
        let path = dir.join("adapter");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        crate::extension::load(&dir).unwrap()
    }

    fn rows() -> Vec<RowIn> {
        vec![RowIn {
            key: "local/b1".into(),
            host: "local".into(),
            session: "api".into(),
            tab: "build".into(),
            pane: "seat-a".into(),
            title: String::new(),
            process: None,
            harness: None,
            state: "unknown".into(),
            source: "inferred".into(),
        }]
    }

    #[test]
    fn the_adapter_gets_the_request_and_answers_per_row() {
        let root = tempfile::tempdir().unwrap();
        let seen = root.path().join("seen.json");
        let script = format!(
            "cat > '{}'\necho '{{\"contract\": 1, \"rows\": {{\"local/b1\": {{\"fields\": {{\"rig\": \"api\"}}}}}}}}'",
            seen.display()
        );
        let ext = adapter(root.path(), &script, "");
        let settings = serde_json::json!({ "workspace": "~/work" });
        let parsed = run_once(&ext, &settings, &rows(), &|| false).unwrap();
        assert_eq!(parsed.rows["local/b1"].fields["rig"], "api");
        let request: Value = serde_json::from_slice(&std::fs::read(seen).unwrap()).unwrap();
        assert_eq!(request["contract"], 1);
        assert_eq!(request["extension"], "fake");
        assert_eq!(request["settings"], settings);
        assert_eq!(request["rows"][0]["key"], "local/b1");
        assert_eq!(request["rows"][0]["tab"], "build");
    }

    #[test]
    fn the_shipped_example_names_each_rows_project_from_its_session() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/extensions/session-prefix");
        let ext = crate::extension::load(&dir).unwrap();
        let mut rows = rows();
        rows[0].session = "api-gateway".into();
        let parsed = run_once(&ext, &serde_json::json!({}), &rows, &|| false).unwrap();
        assert_eq!(parsed.problems, Vec::<String>::new());
        assert_eq!(parsed.rows["local/b1"].fields["project"], "api");
        let dot = serde_json::json!({ "separator": "." });
        rows[0].session = "web.app-v2".into();
        let parsed = run_once(&ext, &dot, &rows, &|| false).unwrap();
        assert_eq!(parsed.rows["local/b1"].fields["project"], "web");
    }

    /// What `run_once_with` gave `emit`, and how the run ended.
    fn said(ext: &Extension) -> (Vec<String>, Result<Parsed, String>) {
        let lines = std::sync::Mutex::new(Vec::new());
        let result = run_once_with(ext, &Value::Null, &rows(), &|| false, &|line| {
            lines.lock().unwrap().push(line.to_owned());
        });
        (lines.into_inner().unwrap(), result)
    }

    #[test]
    fn what_an_adapter_said_on_stderr_is_given_out_before_the_run_returns() {
        let root = tempfile::tempdir().unwrap();
        // A failed run and a good one both: a command that ends with the run must not lose it.
        let failing = adapter(
            root.path(),
            "cat >/dev/null; echo oops >&2; echo and more >&2; exit 3",
            "",
        );
        let (lines, result) = said(&failing);
        assert_eq!(lines, ["oops", "and more"]);
        assert!(result.unwrap_err().starts_with("exited with"));
        let again = tempfile::tempdir().unwrap();
        let good = adapter(
            again.path(),
            "cat >/dev/null; echo note >&2; echo '{\"contract\": 1, \"rows\": {}}'",
            "",
        );
        let (lines, result) = said(&good);
        assert_eq!(lines, ["note"]);
        assert!(result.is_ok());
    }

    #[test]
    fn what_a_timed_out_adapter_said_before_it_was_stopped_is_given_out_too() {
        let root = tempfile::tempdir().unwrap();
        let ext = adapter(
            root.path(),
            "echo before >&2; sleep 30",
            "every = 2\ntimeout = 1\n",
        );
        let (lines, result) = said(&ext);
        assert_eq!(lines, ["before"]);
        assert_eq!(result.unwrap_err(), "timed out after 1s and was stopped");
    }

    #[test]
    fn a_hanging_adapter_is_stopped_at_its_timeout() {
        let root = tempfile::tempdir().unwrap();
        let ext = adapter(root.path(), "sleep 30", "every = 2\ntimeout = 1\n");
        let started = Instant::now();
        let err = run_once(&ext, &Value::Null, &rows(), &|| false).unwrap_err();
        assert_eq!(err, "timed out after 1s and was stopped");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn what_an_adapter_leaves_running_is_stopped_with_it() {
        let root = tempfile::tempdir().unwrap();
        let script = "sleep 30 &\ncat > /dev/null\necho '{\"contract\": 1, \"rows\": {}}'";
        let ext = adapter(root.path(), script, "every = 2\ntimeout = 1\n");
        let started = Instant::now();
        assert!(run_once(&ext, &Value::Null, &rows(), &|| false).is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_child_that_leaves_the_group_cannot_hold_the_run() {
        let root = tempfile::tempdir().unwrap();
        // The adapter ends only once its child has left the group (it says so with a file), not
        // after a pause that python's start under load can outlast: a child still in the group
        // is killed with it and holds nothing. The child stops when told, and says so, or ends
        // after 15 s.
        let child = "import os, time\nos.setsid()\nopen('left', 'w').close()\nfor _ in range(300):\n    if os.path.exists('stop'):\n        open('stopped', 'w').close()\n        break\n    time.sleep(0.05)";
        let script = format!(
            "python3 -c \"{child}\" &\ncat > /dev/null\nwhile [ ! -e left ]; do sleep 0.05; done\necho '{{\"contract\": 1, \"rows\": {{}}}}'"
        );
        let ext = adapter(root.path(), &script, "every = 4\ntimeout = 3\n");
        let started = Instant::now();
        let err = run_once(&ext, &Value::Null, &rows(), &|| false).unwrap_err();
        let took = started.elapsed();
        std::fs::write(ext.dir.join("stop"), "").unwrap();
        // The directory goes with the test: wait for the child to have taken the stop, so it
        // does not outlive the fixture looking for a file that is gone.
        let acked = Instant::now() + Duration::from_secs(10);
        while !ext.dir.join("stopped").exists() && Instant::now() < acked {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ext.dir.join("stopped").exists(), "the child took the stop");
        assert_eq!(err, "its output stayed open after it exited");
        assert!(
            took < Duration::from_secs(10),
            "held until its child ended: {took:?}"
        );
    }

    #[test]
    fn stderr_is_kept_to_a_few_kb_and_the_rest_counted() {
        let small = stderr_lines("warning: slow\n\nsecond\n".as_bytes());
        assert_eq!(small, ["warning: slow", "second"]);
        let big = format!("{}\n", "x".repeat(99)).repeat(10_000);
        let lines = stderr_lines(big.as_bytes());
        let kept: usize = lines[..lines.len() - 1].iter().map(String::len).sum();
        assert!(kept <= STDERR_KEEP as usize, "{kept}");
        assert_eq!(
            lines.last().unwrap(),
            &format!(
                "{} more bytes of stderr dropped",
                big.len() - STDERR_KEEP as usize
            )
        );
    }

    #[test]
    fn a_failing_adapter_says_how() {
        let root = tempfile::tempdir().unwrap();
        let ext = adapter(root.path(), "echo oops >&2; exit 3", "");
        let err = run_once(&ext, &Value::Null, &rows(), &|| false).unwrap_err();
        assert!(err.starts_with("exited with"), "{err}");
    }

    #[test]
    fn a_failed_run_keeps_the_last_rows_and_a_dropped_runner_stops() {
        let root = tempfile::tempdir().unwrap();
        let ext = adapter(root.path(), "exit 1", "every = 2\ntimeout = 1\n");
        let (tx, rx) = std::sync::mpsc::channel();
        let shared: Rows = Arc::new(Mutex::new(rows()));
        let runner = Runner::spawn(ext, Value::Null, shared, tx);
        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(!first.state.ok && first.results.is_none(), "kept");
        drop(runner);
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_err(), "stopped");
    }
}
