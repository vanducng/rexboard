use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// `config.toml`. Every key is optional, and a missing file is an empty one.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The Rex app client a jump selects the session in, by label or id.
    pub client: Option<String>,
    /// `[jump]` held `pre_cmd`, which went with the window command; an old one is ignored.
    #[serde(default, rename = "jump")]
    _jump: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    pub hosts: Hosts,
    #[serde(default)]
    pub extensions: Extensions,
    /// `[models]`: a reported model name, exactly, and what the board shows for it.
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    /// `[[mark]]`: rules for the rows to mark. Kept as read, so a rule that does not fit is
    /// skipped and named by `Marks::new`, not a reason for the whole file to fail.
    #[serde(default)]
    pub mark: Option<toml::Value>,
}

/// `[extensions]`: which discovered extensions run, where else to look, and what each is told.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extensions {
    /// Only these run; a directory dropped in runs nothing until it is named here.
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Roots to look in after the config dir's `extensions`, with `~/` for home.
    #[serde(default)]
    pub dirs: Vec<String>,
    /// `[extensions.settings.NAME]`, passed to that extension untouched.
    #[serde(default)]
    pub settings: BTreeMap<String, toml::Table>,
}

impl Extensions {
    pub fn dirs(&self) -> Vec<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.dirs
            .iter()
            .map(|dir| match (dir.strip_prefix("~/"), &home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ => PathBuf::from(dir),
            })
            .collect()
    }

    /// An extension's settings as JSON, `{}` when it has none.
    pub fn settings(&self, name: &str) -> serde_json::Value {
        self.settings
            .get(name)
            .and_then(|table| serde_json::to_value(table).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hosts {
    /// `rex hosts` labels rexboard leaves out.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// `$XDG_CONFIG_HOME/rexboard/config.toml`, else `~/.config/rexboard/config.toml`.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("rexboard/config.toml"))
}

/// A config.toml that could not be read: what `load` fails with, and how serve says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bad {
    /// For the board's top bar: `config.toml bad: line 12`.
    pub short: String,
    /// For a warning: the file, the line and what is wrong.
    pub why: String,
    /// The `CONFIG` error's message, which the commands that read the file themselves print.
    #[serde(skip)]
    error: String,
}

pub fn load() -> Result<Config> {
    try_load().map_err(|bad| Error::Config(bad.error))
}

pub fn try_load() -> std::result::Result<Config, Bad> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(err) => {
            let why = format!("could not read {}: {err}", path.display());
            return Err(Bad {
                short: "config.toml unreadable".to_owned(),
                error: why.clone(),
                why,
            });
        }
    };
    toml::from_str(&text).map_err(|err| {
        let line = err
            .span()
            .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
        let (short, at) = match line {
            Some(line) => (
                format!("config.toml bad: line {line}"),
                format!(" line {line}:"),
            ),
            None => ("config.toml bad".to_owned(), String::new()),
        };
        Bad {
            short,
            why: format!("{}:{at} {}", path.display(), err.message()),
            error: format!("{}: {}", path.display(), err.message()),
        }
    })
}
