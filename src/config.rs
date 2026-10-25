use std::path::PathBuf;

use serde::Deserialize;

use crate::error::{Error, Result};

/// `config.toml`. Every key is optional, and a missing file is an empty one.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The Rex app client a jump selects the session in, by label or id.
    pub client: Option<String>,
    #[serde(default)]
    pub jump: Jump,
    #[serde(default)]
    pub hosts: Hosts,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jump {
    /// Run before the app selects the session, since it selects in its focused window.
    pub pre_cmd: Option<Vec<String>>,
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
