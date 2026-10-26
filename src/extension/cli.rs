//! `rexboard ext`: list the extensions, run one once against real rows, and start a new one
//! from the example.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use super::contract::{Parsed, RowIn};
use super::{Discovery, discover, is_name, roots};
use crate::board::Row;
use crate::cli::ExtCommand;
use crate::error::{Error, Result};

const EXAMPLE_MANIFEST: &str =
    include_str!("../../examples/extensions/session-prefix/extension.toml");
const EXAMPLE_ADAPTER: &str = include_str!("../../examples/extensions/session-prefix/adapter");
const DOC: &str = "https://github.com/vanducng/rexboard/blob/main/docs/extensions.md";

pub fn run(command: &ExtCommand, json: bool) -> Result<()> {
    match command {
        ExtCommand::Ls => ls(json),
        ExtCommand::Test { name, rows } => test(name, rows.as_deref(), json),
        ExtCommand::New { name } => new(name, json),
    }
}

/// One extension as `ext ls` shows it.
struct Entry {
    name: String,
    contract: Option<u32>,
    enabled: bool,
    dir: Option<PathBuf>,
    /// How its last run in serve went: `ok 120ms`, or the error.
    last: Option<String>,
    /// Why it cannot run: broken, or enabled but found nowhere.
    problem: Option<String>,
    icon: Option<String>,
}

fn ls(json: bool) -> Result<()> {
    let config = crate::config::load()?.extensions;
    let found = discover(&roots(&config.dirs()));
    let serve = crate::client::ask_running(&json!({ "cmd": "rows" }))?;
    let runs = serve
        .as_ref()
        .and_then(|reply| reply.get("extensions"))
        .cloned()
        .unwrap_or_default();
    let entries = entries(&found, &config.enabled, &runs);
    if json {
        let list: Vec<Value> = entries
            .iter()
            .map(|e| {
                json!({
                    "name": e.name, "contract": e.contract, "enabled": e.enabled,
                    "dir": e.dir, "last_run": runs.get(&e.name),
                    "problem": e.problem, "icon": e.icon,
                })
            })
            .collect();
        println!("{}", Value::Array(list));
        return Ok(());
    }
    if entries.is_empty() {
        let looked: Vec<String> = roots(&config.dirs()).iter().map(|r| shown(r)).collect();
        println!(
            "no extensions in {}; rexboard ext new NAME starts one",
            looked.join(", ")
        );
        return Ok(());
    }
    if serve.is_none() && entries.iter().any(|e| e.enabled) {
        eprintln!("serve is not running, so there is no last run to show");
    }
    print!("{}", table(&entries));
    Ok(())
}

/// The valid extensions, then the broken ones, then names enabled but found nowhere.
fn entries(found: &Discovery, enabled: &[String], runs: &Value) -> Vec<Entry> {
    let on = |name: &str| enabled.iter().any(|e| e == name);
    let mut entries: Vec<Entry> = found
        .found
        .values()
        .map(|ext| Entry {
            name: ext.name().to_owned(),
            contract: Some(ext.manifest.contract),
            enabled: on(ext.name()),
            dir: Some(ext.dir.clone()),
            last: runs.get(ext.name()).map(last_run),
            problem: None,
            icon: ext.manifest.icon.clone(),
        })
        .collect();
    for problem in &found.problems {
        let name = problem.dir.file_name().map_or_else(
            || problem.dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        entries.push(Entry {
            enabled: on(&name),
            name,
            contract: None,
            dir: Some(problem.dir.clone()),
            last: None,
            problem: Some(format!("broken: {}", problem.why)),
            icon: None,
        });
    }
    for name in enabled {
        if !entries.iter().any(|e| &e.name == name) {
            entries.push(Entry {
                name: name.clone(),
                contract: None,
                enabled: true,
                dir: None,
                last: None,
                problem: Some("enabled, but not found".to_owned()),
                icon: None,
            });
        }
    }
    entries
}

fn last_run(state: &Value) -> String {
    match state.get("error").and_then(Value::as_str) {
        Some(why) => why.to_owned(),
        None => format!(
            "ok {}ms",
            state.get("ms").and_then(Value::as_u64).unwrap_or(0)
        ),
    }
}

fn table(entries: &[Entry]) -> String {
    let rows: Vec<[String; 5]> = entries
        .iter()
        .map(|e| {
            [
                e.name.clone(),
                e.contract.map_or_else(|| "-".to_owned(), |c| c.to_string()),
                if e.enabled { "yes" } else { "no" }.to_owned(),
                e.problem
                    .clone()
                    .or_else(|| e.last.clone())
                    .unwrap_or_else(|| "-".to_owned()),
                e.dir.as_deref().map_or_else(|| "-".to_owned(), shown),
            ]
        })
        .collect();
    let head = ["NAME", "CONTRACT", "ENABLED", "LAST RUN", "DIR"].map(str::to_owned);
    let mut widths = head.clone().map(|h| h.chars().count());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&head).chain(&rows) {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if i + 1 == row.len() {
                line.push_str(cell);
            } else {
                line.push_str(&format!("{cell:<w$}  ", w = widths[i]));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// A path with the home directory as `~`.
fn shown(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn test(name: &str, rows: Option<&Path>, json: bool) -> Result<()> {
    let config = crate::config::load()?.extensions;
    let found = discover(&roots(&config.dirs()));
    let Some(ext) = found.found.get(name) else {
        return Err(
            match found.problems.iter().find(|p| p.dir.ends_with(name)) {
                Some(problem) => Error::extension(problem.to_string(), &[DOC]),
                None => Error::extension(
                    format!("no extension named {name}"),
                    &["rexboard ext ls", &format!("rexboard ext new {name}")],
                ),
            },
        );
    };
    let rows = match rows {
        Some(file) => read_rows(file)?,
        None => crate::client::all_rows()?,
    };
    let sent: Vec<RowIn> = rows.iter().map(RowIn::from).collect();
    let started = Instant::now();
    let result = super::runner::run_once(ext, &config.settings(name), &sent, &|| false);
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let parsed = match result {
        Ok(parsed) => parsed,
        Err(why) => {
            return Err(Error::extension(
                format!("{name}: {why}"),
                &[&format!("rexboard ext test {name}"), DOC],
            ));
        }
    };
    let enabled = config.enabled.iter().any(|e| e == name);
    if json {
        println!("{}", answer_json(name, &parsed, sent.len(), ms));
    } else {
        print!("{}", answer_text(name, enabled, &parsed, sent.len(), ms));
    }
    if parsed.problems.is_empty() {
        return Ok(());
    }
    let count = parsed.problems.len();
    Err(Error::extension(
        format!(
            "{name}'s answer breaks contract 1 in {count} way{}",
            if count == 1 { "" } else { "s" }
        ),
        &[DOC],
    ))
}

/// Rows as `rexboard ls --json` prints them.
fn read_rows(file: &Path) -> Result<Vec<Row>> {
    let text = std::fs::read_to_string(file).map_err(|err| {
        Error::usage_with(format!("could not read {}: {err}", file.display()), &[])
    })?;
    serde_json::from_str(&text).map_err(|err| {
        Error::usage_with(
            format!("{} is not rexboard ls --json output: {err}", file.display()),
            &["rexboard ls --json > rows.json"],
        )
    })
}

fn answer_text(name: &str, enabled: bool, parsed: &Parsed, sent: usize, ms: u64) -> String {
    let off = if enabled { "" } else { " (not enabled)" };
    let mut out = format!(
        "{name}{off}: answered in {ms}ms, fields for {} of {sent} rows\n",
        parsed
            .rows
            .values()
            .filter(|i| !i.fields.is_empty())
            .count()
    );
    for (key, item) in &parsed.rows {
        let mut parts: Vec<String> = item
            .fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        if let Some(status) = &item.status {
            parts.push(format!("status={}", status.state));
        }
        if !parts.is_empty() {
            out.push_str(&format!("  {key}  {}\n", parts.join("  ")));
        }
    }
    for problem in &parsed.problems {
        out.push_str(&format!("problem: {problem}\n"));
    }
    out
}

fn answer_json(name: &str, parsed: &Parsed, sent: usize, ms: u64) -> Value {
    let rows: BTreeMap<&String, Value> = parsed
        .rows
        .iter()
        .map(|(key, item)| (key, json!({ "fields": item.fields, "status": item.status })))
        .collect();
    json!({
        "ok": parsed.problems.is_empty(), "extension": name, "ms": ms, "sent": sent,
        "rows": rows, "problems": parsed.problems,
    })
}

fn new(name: &str, json: bool) -> Result<()> {
    if !is_name(name) {
        return Err(Error::usage_with(
            format!("an extension's name is [a-z0-9-]+, not {name:?}"),
            &["rexboard ext new my-tool"],
        ));
    }
    let root = roots(&[])
        .into_iter()
        .next()
        .ok_or_else(|| Error::extension("no config dir: set HOME or XDG_CONFIG_HOME", &[]))?;
    let dir = root.join(name);
    if dir.exists() {
        return Err(Error::extension(
            format!("{} is already there", shown(&dir)),
            &[&format!("rexboard ext test {name}")],
        ));
    }
    std::fs::create_dir_all(&dir)?;
    let manifest = EXAMPLE_MANIFEST.replacen(
        "name = \"session-prefix\"",
        &format!("name = \"{name}\""),
        1,
    );
    std::fs::write(dir.join("extension.toml"), manifest)?;
    let adapter = dir.join("adapter");
    std::fs::write(&adapter, EXAMPLE_ADAPTER)?;
    std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o755))?;
    if json {
        println!("{}", json!({ "ok": true, "name": name, "dir": dir }));
        return Ok(());
    }
    let config = crate::config::path().map_or_else(|| "config.toml".to_owned(), |p| shown(&p));
    println!(
        "created {}\nnext:\n  edit {}\n  rexboard ext test {name}\n  add \"{name}\" to extensions.enabled in {config}",
        shown(&dir),
        shown(&adapter)
    );
    Ok(())
}
