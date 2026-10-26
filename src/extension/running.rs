//! The enabled extensions in serve: which run, the rows they are sent, and how each last went.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use super::contract::RowIn;
use super::runner::{Report, Rows, RunState, Runner};
use super::{Extension, discover, field_names, roots};
use crate::board::Row;

pub struct Running {
    runners: BTreeMap<String, Runner>,
    pub states: BTreeMap<String, RunState>,
    rows: Rows,
    tx: Sender<Report>,
    /// Problems already logged, so a rescan does not repeat them.
    said: BTreeSet<String>,
}

impl Running {
    pub fn new(tx: Sender<Report>) -> Self {
        Self {
            runners: BTreeMap::new(),
            states: BTreeMap::new(),
            rows: Arc::new(Mutex::new(Vec::new())),
            tx,
            said: BTreeSet::new(),
        }
    }

    /// Reads config.toml and the extension roots again: runs what is newly enabled, stops what
    /// is no longer, and restarts one whose manifest or settings changed. Returns the names that
    /// stopped for good, whose fields and status should leave the rows.
    pub fn sync(&mut self) -> Vec<String> {
        let config = match crate::config::load() {
            Ok(config) => config.extensions,
            Err(err) => {
                self.say(format!("ext: config not read, extensions unchanged: {err}"));
                return Vec::new();
            }
        };
        let found = discover(&roots(&config.dirs()));
        for problem in found.problems {
            self.say(problem);
        }
        let mut want: BTreeMap<String, (Extension, Value)> = BTreeMap::new();
        for name in &config.enabled {
            match found.found.get(name) {
                Some(ext) => {
                    want.insert(name.clone(), (ext.clone(), config.settings(name)));
                }
                None => self.say(format!(
                    "ext {name}: enabled, but no valid extension of that name was found"
                )),
            }
        }
        let stopped: Vec<String> = self
            .runners
            .keys()
            .filter(|name| !want.contains_key(*name))
            .cloned()
            .collect();
        self.runners.retain(|name, runner| {
            want.get(name)
                .is_some_and(|(ext, settings)| *ext == runner.ext && *settings == runner.settings)
        });
        self.states.retain(|name, _| want.contains_key(name));
        for (name, (ext, settings)) in want {
            if !self.runners.contains_key(&name) {
                let runner = Runner::spawn(ext, settings, self.rows.clone(), self.tx.clone());
                self.runners.insert(name, runner);
            }
        }
        stopped
    }

    fn say(&mut self, line: String) {
        if self.said.insert(line.clone()) {
            eprintln!("{line}");
        }
    }

    /// The rows the next run of each extension is sent.
    pub fn set_rows(&self, rows: &[Row]) {
        let rows: Vec<RowIn> = rows.iter().map(RowIn::from).collect();
        *self.rows.lock().unwrap_or_else(PoisonError::into_inner) = rows;
    }

    /// Whether `name` is still running and may give rows a status.
    pub fn provides_status(&self, name: &str) -> Option<bool> {
        self.runners
            .get(name)
            .map(|runner| runner.ext.manifest.provides.status)
    }

    /// The declared fields of the running extensions, by id, with the name the board shows and
    /// the column title.
    pub fn fields(&self) -> BTreeMap<String, Value> {
        let exts: Vec<&Extension> = self.runners.values().map(|runner| &runner.ext).collect();
        let names = field_names(&exts);
        let mut fields = BTreeMap::new();
        for ext in exts {
            for field in &ext.manifest.fields {
                let id = super::field_id(ext.name(), &field.name);
                let shown = names[&id].clone();
                fields.insert(id, json!({ "name": shown, "title": field.title }));
            }
        }
        fields
    }
}
