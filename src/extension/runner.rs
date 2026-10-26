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

/// Runs the adapter once on `rows`, within the manifest's timeout, and reads its answer.
pub fn run_once(
    ext: &Extension,
    settings: &Value,
    rows: &[RowIn],
    stopped: &dyn Fn() -> bool,
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
    let writer = thread::spawn(move || stdin.write_all(&input));
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout
            .by_ref()
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut out);
        out
    });
    let stderr = child.stderr.take().expect("piped stderr");
    thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::BufReader::new(stderr).read_to_string(&mut text);
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            eprintln!("ext {name}: {line}");
        }
    });
    let limit = Duration::from_secs(ext.manifest.timeout);
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break status;
        }
        let late = Instant::now() >= deadline;
        if late || stopped() {
            let pid = child.id() as i32;
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = child.wait();
            return Err(if late {
                format!("timed out after {}s and was stopped", limit.as_secs())
            } else {
                "stopped".to_owned()
            });
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("exited with {status}"));
    }
    let keys = rows.iter().map(|row| row.key.clone()).collect();
    contract::parse(&out, ext, &keys)
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
