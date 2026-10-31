use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::board::{Board, Group, Memory};
use crate::collector::{self, Collector, HostEvent, HostId};
use crate::error::{Error, Result};
use crate::event::Event;
use crate::extension::runner::Report;
use crate::extension::running::Running;
use crate::harness::usage::{self, Usage};
use crate::state::{self, Lock, Paths, Record};
use crate::{interrupted, rex, stop_on_signals};

pub const VERSION: u64 = 1;

/// This build's version. Debug builds take REXBOARD_TEST_VERSION, so tests can stand up an
/// older or newer serve without a second build.
pub fn version() -> String {
    if cfg!(debug_assertions)
        && let Ok(version) = std::env::var("REXBOARD_TEST_VERSION")
    {
        return version;
    }
    env!("CARGO_PKG_VERSION").to_owned()
}
const DEBOUNCE: Duration = Duration::from_millis(100);
const SAVE_EVERY: Duration = Duration::from_secs(1);
const HEARTBEAT: Duration = Duration::from_secs(1);
const SUBSCRIBER_BUFFER: usize = 4;
const RESCAN: Duration = Duration::from_secs(300);

/// How often serve rescans. Debug builds take REXBOARD_TEST_RESCAN_SECS, so a test can see a
/// rescan without waiting five minutes.
fn rescan_every() -> Duration {
    if cfg!(debug_assertions)
        && let Some(secs) = std::env::var("REXBOARD_TEST_RESCAN_SECS")
            .ok()
            .and_then(|secs| secs.parse().ok())
    {
        return Duration::from_secs(secs);
    }
    RESCAN
}
/// `rows` waits this long at most for every host's first snapshot after serve starts.
const READY_WITHIN: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    Rows,
    Subscribe,
    Seen {
        key: String,
        /// Set by a jump, so `next` outside a pane starts after the pane it went to.
        #[serde(default)]
        jump: bool,
    },
    Next {
        after: Option<String>,
        #[serde(default)]
        back: bool,
    },
    /// The pane the jump before the last one went to, for `jump -`.
    Back,
    Stop,
    /// Which rexboard this serve is, so a newer client can replace it.
    Version,
}

enum Msg {
    Host(HostId, HostEvent),
    Ext(Report),
    /// Each local row's usage, by row key, as the usage files say now.
    Usage(BTreeMap<String, Usage>),
    Hosts(Vec<String>),
    Request(Request, SyncSender<String>),
}

/// Runs serve in this process until `stop`, SIGINT or SIGTERM.
pub fn run() -> Result<()> {
    let paths = Paths::find()?;
    paths.create()?;
    let Some((mut lock, previous)) = Lock::take(&paths.lock())? else {
        return Err(Error::ServeRunning(format!(
            "serve is already running (lock {})",
            paths.lock().display()
        )));
    };
    // Recorded before anything slow, so serve --stop never finds an old pid under a held lock.
    lock.write(&Record {
        pid: std::process::id() as i32,
        ..Record::default()
    })?;
    state::sweep(&previous);
    state::roll(&paths.transitions());
    let rex_path = rex::locate()?.display().to_string();
    let record = |children: Vec<i32>| Record {
        pid: std::process::id() as i32,
        rex: rex_path.clone(),
        children,
    };
    lock.write(&record(Vec::new()))?;
    let _ = std::fs::remove_file(&paths.socket);
    let listener = {
        let _guard = rex::fd_guard();
        UnixListener::bind(&paths.socket)
    }
    .map_err(|err| Error::serve(format!("could not bind {}: {err}", paths.socket.display())))?;
    std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    stop_on_signals();
    eprintln!(
        "serve {} listening on {}",
        std::process::id(),
        paths.socket.display()
    );

    let (tx, rx) = mpsc::channel();
    let accept_tx = tx.clone();
    thread::spawn(move || accept(&listener, &accept_tx));
    let mut reducer = Reducer::new(state::load(&paths.state()), tx);
    reducer.rescan_now();
    let result = reducer.run(&rx, &paths, &mut lock, &record);
    drop(reducer);
    let _ = std::fs::remove_file(&paths.socket);
    let _ = lock.write(&record(Vec::new()));
    eprintln!("serve {} stopped", std::process::id());
    result
}

fn accept(listener: &UnixListener, tx: &Sender<Msg>) {
    loop {
        // Polled so the fd lock is held per attempt, never across a blocking accept.
        let accepted = {
            let _guard = rex::fd_guard();
            listener.accept()
        };
        match accepted {
            Ok((stream, _)) => {
                let tx = tx.clone();
                thread::spawn(move || connection(stream, &tx));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                eprintln!("serve: accept failed: {err}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn connection(stream: UnixStream, tx: &Sender<Msg>) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(err) => {
                let reply = error_line("USAGE", &format!("bad request: {err}"));
                if out.write_all(format!("{reply}\n").as_bytes()).is_err() {
                    return;
                }
                continue;
            }
        };
        if matches!(request, Request::Version) {
            let reply = json!({ "v": VERSION, "rexboard": version() });
            if out.write_all(format!("{reply}\n").as_bytes()).is_err() {
                return;
            }
            continue;
        }
        let streaming = matches!(request, Request::Subscribe);
        // Bounded: a subscriber that stops reading loses lines instead of growing serve.
        let (reply_tx, reply_rx) = mpsc::sync_channel(SUBSCRIBER_BUFFER);
        if tx.send(Msg::Request(request, reply_tx)).is_err() {
            return;
        }
        for reply in reply_rx.iter() {
            // One write per line: serve may exit right after a stop reply, and a client must
            // never see the JSON without its newline.
            if out.write_all(format!("{reply}\n").as_bytes()).is_err() {
                return;
            }
            if !streaming {
                break;
            }
        }
        if streaming {
            return;
        }
    }
}

fn error_line(code: &str, message: &str) -> Value {
    json!({ "v": VERSION, "error": { "code": code, "message": message } })
}

/// Why a host is down, as kept and sent with every rows line: cut as the log cuts a bad line,
/// since a dying observer's last stderr line can be any length.
fn capped(reason: &str) -> String {
    reason.chars().take(500).collect()
}

struct Reducer {
    board: Board,
    memory: BTreeMap<String, Memory>,
    collectors: BTreeMap<HostId, Collector>,
    hosts: BTreeMap<HostId, &'static str>,
    /// Why a host is down or stopped, until it is up again.
    host_errors: BTreeMap<HostId, String>,
    restored: BTreeSet<HostId>,
    host_tx: Sender<(HostId, HostEvent)>,
    tx: Sender<Msg>,
    subscribers: Subscribers,
    /// Requests that need every host's first snapshot, answered once it is in.
    waiting: Vec<(Request, SyncSender<String>)>,
    started: Instant,
    announced: bool,
    rows_cache: Option<String>,
    suspects: BTreeSet<(String, String)>,
    /// The keys the last two jumps went to, older first, kept in state.json.
    jumps: Vec<String>,
    rows_due: Option<Instant>,
    save_due: Option<Instant>,
    extensions: Running,
    /// `[models]` from config.toml, read again with the extensions.
    models: BTreeMap<String, String>,
    usage: BTreeMap<String, Usage>,
    /// The local rows the usage watcher joins its files to.
    usage_panes: Arc<Mutex<Option<Vec<UsagePane>>>>,
}

/// A row on serve's own host: its usage file is named for its block, or for the tmux pane its
/// tmux client shows.
#[derive(Clone)]
struct UsagePane {
    row: String,
    block: String,
    /// The foreground process, which is the tmux client or a wrapper around it.
    front: Option<u32>,
    tmux_client: Option<u32>,
}

/// Each pane's tmux client, found among the children of the wrapper it runs under when the
/// foreground process is not tmux itself.
fn with_clients(mut panes: Vec<UsagePane>, procs: &usage::Procs) -> Vec<UsagePane> {
    for pane in &mut panes {
        if pane.tmux_client.is_none() {
            pane.tmux_client = pane.front.and_then(|front| procs.tmux_child(front));
        }
    }
    panes
}

/// How often the usage files and tmux clients are read again.
const USAGE_EVERY: Duration = Duration::from_secs(2);

/// Reads the usage files off the reducer's thread, since joining a tmux seat runs `ps` and
/// `tmux`, and sends each local row's usage whenever it is read. `panes` is None until this
/// host's first snapshot, and while it is, no file is removed.
fn watch_usage(
    state: std::path::PathBuf,
    panes: Arc<Mutex<Option<Vec<UsagePane>>>>,
    tx: Sender<Msg>,
) {
    thread::spawn(move || {
        let mut files = usage::Files::default();
        loop {
            thread::sleep(USAGE_EVERY);
            let known = panes.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let mut panes = known.clone().unwrap_or_default();
            let any = usage::any_tmux(&state);
            if any {
                panes = with_clients(panes, &usage::Procs::read());
            }
            let pids: Vec<u32> = panes.iter().filter_map(|p| p.tmux_client).collect();
            let asked = !pids.is_empty() && any;
            let tmux = if asked {
                usage::tmux_keys(&pids)
            } else {
                usage::Tmux::default()
            };
            let live: BTreeSet<String> = panes
                .iter()
                .map(|p| p.block.clone())
                .chain(tmux.shown.values().cloned())
                .chain(tmux.panes.iter().cloned())
                .collect();
            // A tmux client that did not answer this time may show a live pane: keep every file.
            let whole = known.is_some() && (!asked || tmux.shown.len() == pids.len());
            let kept = files.read(&state, whole.then_some(&live), chrono::Utc::now());
            let by_row = panes
                .iter()
                .filter_map(|pane| {
                    let key = match pane.tmux_client {
                        Some(pid) => tmux.shown.get(&pid)?,
                        None => &pane.block,
                    };
                    Some((pane.row.clone(), kept.get(key)?.clone()))
                })
                .collect();
            if tx.send(Msg::Usage(by_row)).is_err() {
                return;
            }
        }
    });
}

impl Reducer {
    fn new(saved: state::Saved, tx: Sender<Msg>) -> Self {
        let (host_tx, host_rx) = mpsc::channel::<(HostId, HostEvent)>();
        let forward = tx.clone();
        thread::spawn(move || {
            for (host, event) in host_rx {
                if forward.send(Msg::Host(host, event)).is_err() {
                    return;
                }
            }
        });
        let (ext_tx, ext_rx) = mpsc::channel::<Report>();
        let forward = tx.clone();
        thread::spawn(move || {
            for report in ext_rx {
                if forward.send(Msg::Ext(report)).is_err() {
                    return;
                }
            }
        });
        Self {
            board: Board::default(),
            memory: saved.panes,
            collectors: BTreeMap::new(),
            hosts: BTreeMap::new(),
            host_errors: BTreeMap::new(),
            restored: BTreeSet::new(),
            host_tx,
            tx,
            subscribers: Subscribers::default(),
            waiting: Vec::new(),
            started: Instant::now(),
            announced: false,
            rows_cache: None,
            suspects: BTreeSet::new(),
            jumps: saved.jumps,
            rows_due: None,
            save_due: None,
            extensions: Running::new(ext_tx),
            models: BTreeMap::new(),
            usage: BTreeMap::new(),
            usage_panes: Arc::default(),
        }
    }

    fn run(
        &mut self,
        rx: &Receiver<Msg>,
        paths: &Paths,
        lock: &mut Lock,
        record: &dyn Fn(Vec<i32>) -> Record,
    ) -> Result<()> {
        let mut beat = Instant::now();
        let mut rescanned = Instant::now();
        let mut children = Vec::new();
        watch_usage(
            paths.dir.clone(),
            Arc::clone(&self.usage_panes),
            self.tx.clone(),
        );
        while !interrupted() {
            let now = Instant::now();
            if self.rows_due.is_some_and(|due| now >= due) {
                self.rows_due = None;
                self.subscribers.owe_all();
            }
            if self.subscribers.owed() {
                // Serialized once per change, not on every pass while a reader is owed rows.
                if self.rows_cache.is_none() {
                    self.rows_cache = Some(self.rows_line().to_string());
                }
                let line = self.rows_cache.as_deref().unwrap_or_default();
                self.subscribers.pay(line);
            }
            if self.save_due.is_some_and(|due| now >= due) {
                self.save_due = None;
                if let Err(err) = state::save(&paths.state(), &self.memory_to_save(), &self.jumps) {
                    eprintln!("serve: could not save state: {err}");
                }
            }
            if now.duration_since(beat) >= HEARTBEAT {
                beat = now;
                // A working report goes suspect with time alone, with no event to say so.
                let suspects = self.board.suspects(chrono::Utc::now());
                if suspects != self.suspects {
                    self.suspects = suspects;
                    self.rows_changed();
                }
                let line = json!({ "v": VERSION, "heartbeat": true }).to_string();
                self.subscribers.heartbeat(&line);
                *self
                    .usage_panes
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) =
                    self.restored.contains("local").then(|| self.local_panes());
                let pids: Vec<i32> = self
                    .collectors
                    .values()
                    .map(Collector::pid)
                    .filter(|&p| p > 0)
                    .collect();
                if pids != children {
                    children = pids;
                    lock.write(&record(children.clone()))?;
                }
            }
            if now.duration_since(rescanned) >= rescan_every() {
                rescanned = now;
                self.rescan();
            }
            if !self.announced && self.ready() {
                // Subscribers that came before every host reported get rows now.
                self.announced = true;
                self.subscribers.owe_all();
                for (request, reply) in std::mem::take(&mut self.waiting) {
                    self.request(request, reply);
                }
            }
            let msg = match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            match msg {
                Msg::Host(host, event) => self.host_event(&host, &event, paths),
                Msg::Ext(report) => self.ext_report(report, paths),
                Msg::Usage(usage) => {
                    if usage != self.usage {
                        self.usage = usage;
                        self.rows_changed();
                    }
                }
                Msg::Hosts(labels) => self.set_hosts(&labels),
                Msg::Request(Request::Stop, reply) => {
                    offer(&reply, &json!({ "v": VERSION, "ok": true }).to_string());
                    break;
                }
                Msg::Request(request, reply) => self.request(request, reply),
            }
        }
        state::save(&paths.state(), &self.memory_to_save(), &self.jumps)
    }

    /// The board's memory, plus what was loaded for hosts whose first snapshot has not come,
    /// so a host offline for a whole run keeps its seen and since for the next one.
    fn memory_to_save(&self) -> BTreeMap<String, Memory> {
        let mut memory: BTreeMap<String, Memory> = self
            .memory
            .iter()
            .filter(|(key, _)| {
                key.split_once('/')
                    .is_none_or(|(host, _)| !self.restored.contains(host))
            })
            .map(|(key, m)| (key.clone(), m.clone()))
            .collect();
        memory.extend(self.board.memory());
        memory
    }

    fn rescan_now(&mut self) {
        self.sync_extensions();
        match collector::hosts() {
            Ok(labels) => self.set_hosts(&labels),
            Err(err) => {
                eprintln!("serve: rex hosts failed, watching this host only: {err}");
                self.set_hosts(&[]);
            }
        }
    }

    /// `rex hosts` can be slow over the network, so later scans run off the reducer thread.
    fn rescan(&mut self) {
        self.sync_extensions();
        let tx = self.tx.clone();
        thread::spawn(move || match collector::hosts() {
            Ok(labels) => {
                let _ = tx.send(Msg::Hosts(labels));
            }
            Err(err) => eprintln!("serve: rex hosts failed, keeping the current hosts: {err}"),
        });
    }

    fn set_hosts(&mut self, labels: &[String]) {
        self.rows_cache = None;
        let mut want: Vec<HostId> = vec!["local".into()];
        want.extend(labels.iter().cloned());
        let gone: Vec<HostId> = self
            .collectors
            .keys()
            .filter(|h| !want.contains(h))
            .cloned()
            .collect();
        for host in gone {
            self.collectors.remove(&host);
            self.hosts.remove(&host);
            self.host_errors.remove(&host);
            if !self.board.forget(&host).is_empty() {
                self.rows_changed();
            }
        }
        for host in want {
            if !self.collectors.contains_key(&host) {
                let target = (host != "local").then(|| host.clone());
                self.collectors
                    .insert(host.clone(), Collector::spawn(target, self.host_tx.clone()));
                self.hosts.insert(host, "starting");
            }
        }
    }

    fn host_event(&mut self, host: &str, event: &HostEvent, paths: &Paths) {
        self.rows_cache = None;
        if !self.hosts.contains_key(host) || self.hosts[host] == "stopped" {
            return;
        }
        let changed = match event {
            HostEvent::Failed(err) => self.stop_host(host, err),
            event => match self.board.apply(host, event, chrono::Utc::now()) {
                Ok(mut changed) => {
                    self.note_host(host, event, &mut changed);
                    changed
                }
                Err(err) => {
                    if let Some(collector) = self.collectors.get(host) {
                        collector.restart(host, &err, event);
                    }
                    Vec::new()
                }
            },
        };
        for warning in self.board.take_warnings() {
            eprintln!("{warning}");
        }
        if let Err(err) = state::append(&paths.transitions(), &self.board.take_transitions()) {
            eprintln!("serve: could not append transitions: {err}");
        }
        if !changed.is_empty() {
            self.rows_changed();
        }
    }

    /// A host's state from an event the board took: down, or up on a snapshot, restoring its
    /// remembered rows on the first one.
    fn note_host(&mut self, host: &str, event: &HostEvent, changed: &mut Vec<(String, String)>) {
        match event {
            HostEvent::Down { reason, .. } => {
                self.hosts.insert(host.to_owned(), "down");
                self.host_errors.insert(host.to_owned(), capped(reason));
            }
            HostEvent::Event(Event::Snapshot { .. }, _) => {
                if self.restored.insert(host.to_owned()) {
                    changed.extend(self.board.restore(host, &self.memory));
                }
                self.hosts.insert(host.to_owned(), "up");
                self.host_errors.remove(host);
            }
            _ => {}
        }
    }

    fn stop_host(&mut self, host: &str, err: &Error) -> Vec<(String, String)> {
        eprintln!("warning: {host} stopped and will not restart: {err}");
        self.hosts.insert(host.to_owned(), "stopped");
        self.host_errors
            .insert(host.to_owned(), capped(&err.to_string()));
        self.board.mark_stale(host)
    }

    /// Re-reads the extensions and the model aliases; an extension that stopped takes its fields
    /// and status off the rows, since no later run of it will.
    fn sync_extensions(&mut self) {
        let now = chrono::Utc::now();
        let mut changed = false;
        if let Ok(config) = crate::config::load() {
            changed = std::mem::replace(&mut self.models, config.models) != self.models;
        }
        for name in self.extensions.sync() {
            changed |= !self
                .board
                .extension(&name, &BTreeMap::new(), false, now)
                .is_empty();
        }
        if changed {
            self.rows_changed();
        }
    }

    /// An extension's run: its state for `ext ls` and the board, and its rows when it sent any.
    fn ext_report(&mut self, report: Report, paths: &Paths) {
        self.rows_cache = None;
        let Some(provides_status) = self.extensions.provides_status(&report.name) else {
            return;
        };
        let was = self
            .extensions
            .states
            .insert(report.name.clone(), report.state.clone());
        let flipped =
            was.is_none_or(|was| (was.ok, was.error) != (report.state.ok, report.state.error));
        let changed = report.results.is_some_and(|results| {
            let now = chrono::Utc::now();
            !self
                .board
                .extension(&report.name, &results, provides_status, now)
                .is_empty()
        });
        if let Err(err) = state::append(&paths.transitions(), &self.board.take_transitions()) {
            eprintln!("serve: could not append transitions: {err}");
        }
        if changed || flipped {
            self.rows_changed();
        }
    }

    fn rows_changed(&mut self) {
        self.rows_cache = None;
        self.extensions
            .set_rows(&self.board.rows(chrono::Utc::now()));
        let now = Instant::now();
        self.rows_due.get_or_insert(now + DEBOUNCE);
        self.save_due.get_or_insert(now + SAVE_EVERY);
    }

    /// Every host has sent its first snapshot or failed, or serve has waited long enough.
    fn ready(&self) -> bool {
        self.hosts.values().all(|state| *state != "starting")
            || self.started.elapsed() >= READY_WITHIN
    }

    /// The rows on serve's own host, for the usage watcher.
    fn local_panes(&self) -> Vec<UsagePane> {
        self.board
            .rows(chrono::Utc::now())
            .into_iter()
            .filter(|row| row.pane.host == "local")
            .map(|row| UsagePane {
                front: row.pane.pid,
                tmux_client: row
                    .pane
                    .pid
                    .filter(|_| row.pane.process.as_deref() == Some("tmux")),
                block: row.pane.block_id,
                row: row.key,
            })
            .collect()
    }

    fn rows_line(&mut self) -> Value {
        let actions = self.extensions.actions();
        let mut rows = self.board.rows(chrono::Utc::now());
        for row in &mut rows {
            // The row has what the host's hook told Rex; a local file wins over it, whole.
            if let Some(local) = self.usage.get(&row.key) {
                row.usage = Some(local.clone());
            }
            if let Some(usage) = &mut row.usage {
                usage.alias(&self.models);
            }
        }
        json!({
            "v": VERSION,
            "rows": rows,
            "hosts": self.hosts,
            "host_errors": self.host_errors,
            "extensions": self.extensions.states,
            "fields": self.extensions.fields(),
            "actions": actions,
        })
    }

    fn request(&mut self, request: Request, reply: SyncSender<String>) {
        let line = match request {
            Request::Rows | Request::Back if !self.ready() => {
                self.waiting.push((request, reply));
                return;
            }
            Request::Rows => self.rows_line(),
            Request::Subscribe => {
                self.subscribers.add(reply, self.ready());
                return;
            }
            Request::Seen { key, jump } => match split(&key) {
                Some(row) if self.board.mark_seen(&row) => {
                    if jump && self.jumps.last() != Some(&key) {
                        self.jumps.push(key.clone());
                        let extra = self.jumps.len().saturating_sub(2);
                        self.jumps.drain(..extra);
                    }
                    self.rows_changed();
                    json!({ "v": VERSION, "ok": true, "key": key })
                }
                _ => error_line("NOT_FOUND", &format!("no pane {key}")),
            },
            Request::Next { after, back } => {
                let after = after.or_else(|| self.jumps.last().cloned());
                self.next(after.as_deref(), back)
            }
            Request::Back => self.back(),
            Request::Stop => unreachable!("handled in run"),
            Request::Version => unreachable!("answered by the connection"),
        };
        offer(&reply, &line.to_string());
    }

    /// The row of the jump before the last: going there makes it the last, so a second `jump -`
    /// comes back, as `cd -` does.
    fn back(&self) -> Value {
        let Some(key) = self.jumps.len().checked_sub(2).map(|i| &self.jumps[i]) else {
            return error_line("NOT_FOUND", "no previous jump yet");
        };
        match self
            .board
            .rows(chrono::Utc::now())
            .into_iter()
            .find(|row| row.key == *key)
        {
            Some(row) => json!({ "v": VERSION, "row": row }),
            None => error_line("NOT_FOUND", "previous pane is gone"),
        }
    }

    /// The NEEDS YOU row after `after` (a key, or the block id of the pane asking), unseen
    /// rows first: a jump marks its row seen, so the queue moves on. Seen blocked or erroring
    /// rows still need you, so they cycle once nothing is unseen.
    fn next(&self, after: Option<&str>, back: bool) -> Value {
        let needs: Vec<_> = self
            .board
            .rows(chrono::Utc::now())
            .into_iter()
            .filter(|row| row.group == Group::NeedsYou)
            .collect();
        let mut rows: Vec<_> = needs.iter().filter(|row| !row.seen).cloned().collect();
        if rows.is_empty() {
            rows = needs;
        }
        if rows.is_empty() {
            return error_line("NOBODY_NEEDS_YOU", "nobody needs you");
        }
        if back {
            rows.reverse();
        }
        let at = after.and_then(|id| {
            rows.iter()
                .position(|row| row.key == id || row.pane.block_id == id)
        });
        let pick = at.map_or(0, |i| (i + 1) % rows.len());
        json!({ "v": VERSION, "row": rows[pick] })
    }
}

/// Each subscriber has a few lines of room. A rows line that does not fit stays owed and goes out
/// as soon as there is room, so a reader that stalls still ends on the current rows.
#[derive(Default)]
struct Subscribers {
    subs: Vec<(SyncSender<String>, bool)>,
}

impl Subscribers {
    fn add(&mut self, tx: SyncSender<String>, owed: bool) {
        self.subs.push((tx, owed));
    }

    fn owe_all(&mut self) {
        for (_, owed) in &mut self.subs {
            *owed = true;
        }
    }

    fn owed(&self) -> bool {
        self.subs.iter().any(|(_, owed)| *owed)
    }

    /// Sends `rows` to every subscriber owed it that has room.
    fn pay(&mut self, rows: &str) {
        self.subs.retain_mut(|(tx, owed)| {
            if !*owed {
                return true;
            }
            match tx.try_send(rows.to_owned()) {
                Ok(()) => {
                    *owed = false;
                    true
                }
                Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            }
        });
    }

    /// A subscriber still owed rows gets no heartbeat, so the room goes to the rows.
    fn heartbeat(&mut self, line: &str) {
        self.subs.retain(|(tx, owed)| *owed || offer(tx, line));
    }
}

/// Sends unless the receiver is full; each rows line is a whole snapshot, so a skipped one is
/// replaced by the next. False once the receiver is gone.
fn offer(to: &SyncSender<String>, line: &str) -> bool {
    !matches!(
        to.try_send(line.to_owned()),
        Err(TrySendError::Disconnected(_))
    )
}

fn split(key: &str) -> Option<(String, String)> {
    key.split_once('/')
        .map(|(host, block)| (host.to_owned(), block.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_subscriber_ends_on_the_current_rows() {
        let (tx, rx) = mpsc::sync_channel(SUBSCRIBER_BUFFER);
        let mut subs = Subscribers::default();
        subs.add(tx, true);
        subs.pay("rows 1");
        for _ in 0..SUBSCRIBER_BUFFER {
            subs.heartbeat("beat");
        }
        subs.owe_all();
        subs.pay("rows 2");
        assert!(
            subs.owed(),
            "the reader is stalled, so rows 2 is still owed"
        );
        subs.heartbeat("beat");
        subs.owe_all();
        subs.pay("rows 3");
        let drained: Vec<String> = rx.try_iter().collect();
        assert_eq!(
            drained.len(),
            SUBSCRIBER_BUFFER,
            "the buffer bounds what a stalled reader holds"
        );
        assert_eq!(drained[0], "rows 1");
        subs.heartbeat("beat");
        assert!(rx.try_recv().is_err(), "no heartbeat while rows are owed");
        subs.pay("rows 3");
        assert!(!subs.owed());
        let last: Vec<String> = rx.try_iter().collect();
        assert_eq!(last, ["rows 3"], "the reader resumes on the current rows");
    }

    #[test]
    fn a_pane_under_a_wrapper_gets_the_tmux_client_below_it() {
        let procs =
            usage::Procs::parse("310 100 /bin/sh\n311 310 /usr/bin/tmux\n320 100 /bin/sh\n");
        let pane = |row: &str, front, tmux_client| UsagePane {
            row: row.into(),
            block: format!("block:{row}"),
            front,
            tmux_client,
        };
        let panes = with_clients(
            vec![
                pane("wrapped", Some(310), None),
                pane("bare", Some(320), None),
                pane("direct", Some(900), Some(900)),
                pane("none", None, None),
            ],
            &procs,
        );
        let clients: Vec<_> = panes.iter().map(|p| p.tmux_client).collect();
        assert_eq!(clients, [Some(311), None, Some(900), None]);
    }

    #[test]
    fn a_gone_subscriber_is_dropped() {
        let (tx, rx) = mpsc::sync_channel(SUBSCRIBER_BUFFER);
        let mut subs = Subscribers::default();
        subs.add(tx, true);
        drop(rx);
        subs.pay("rows");
        assert!(subs.subs.is_empty());
    }
}
