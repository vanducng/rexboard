use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

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

pub fn load() -> Result<Config> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(err) => {
            return Err(Error::Config(format!(
                "could not read {}: {err}",
                path.display()
            )));
        }
    };
    toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {}", path.display(), err.message())))
}
