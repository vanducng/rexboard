//! Extensions: a directory with `extension.toml` and an adapter command. rexboard runs the
//! enabled ones over a versioned stdin/stdout contract and knows no tool by name.

pub mod action;
pub mod cli;
pub mod contract;
pub mod runner;
pub mod running;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use action::Action;

/// The contract version this rexboard speaks.
pub const CONTRACT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub contract: u32,
    #[serde(default)]
    pub description: String,
    pub cmd: Vec<String>,
    #[serde(default = "every")]
    pub every: u64,
    #[serde(default = "timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub provides: Provides,
    /// The mark the card gives this extension's fields with `"icons": "nerd"`.
    #[serde(default)]
    pub icon: Option<String>,
    /// What a user can start on a row: a key, a typed line and a confirm.
    #[serde(default)]
    pub actions: Vec<Action>,
}

fn every() -> u64 {
    10
}

fn timeout() -> u64 {
    5
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub title: String,
    /// The field's own mark on the card with `"icons": "nerd"`.
    #[serde(default)]
    pub icon: Option<String>,
    /// A value on the row (for `ls --json` and an action's placeholders) with no column, view,
    /// filter key or card fact.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provides {
    /// The adapter may return a fallback status for rows with no native record.
    #[serde(default)]
    pub status: bool,
}

/// A discovered extension with a valid manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Extension {
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// The adapter's argv, with a leading `./` taken from the extension's directory.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = self.manifest.cmd.clone();
        if let Some(first) = argv.first_mut()
            && let Some(rest) = first.strip_prefix("./")
        {
            *first = self.dir.join(rest).display().to_string();
        }
        argv
    }

    pub fn declares(&self, field: &str) -> bool {
        self.manifest.fields.iter().any(|f| f.name == field)
    }
}

/// What a scan of the extension roots found: the valid extensions by name, and why each bad
/// one was skipped.
#[derive(Debug, Default)]
pub struct Discovery {
    pub found: BTreeMap<String, Extension>,
    pub problems: Vec<Problem>,
}

/// An extension directory that was skipped, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub dir: PathBuf,
    pub why: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.dir.join("extension.toml");
        write!(f, "extension {}: {}", path.display(), self.why)
    }
}

/// Where extensions live: `$XDG_CONFIG_HOME/rexboard/extensions`, then each of `dirs`.
pub fn roots(dirs: &[PathBuf]) -> Vec<PathBuf> {
    crate::config::path()
        .and_then(|config| config.parent().map(|dir| dir.join("extensions")))
        .into_iter()
        .chain(dirs.iter().cloned())
        .collect()
}

/// Each root holds one directory per extension; the first of a name wins.
pub fn discover(roots: &[PathBuf]) -> Discovery {
    let mut discovery = Discovery::default();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.join("extension.toml").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            match read(&dir) {
                Ok(ext) if discovery.found.contains_key(ext.name()) => {
                    discovery.problems.push(Problem {
                        why: format!("shadowed by an earlier extension named {}", ext.name()),
                        dir,
                    });
                }
                Ok(ext) => {
                    discovery.found.insert(ext.name().to_owned(), ext);
                }
                Err(why) => discovery.problems.push(Problem { dir, why }),
            }
        }
    }
    discovery
}

/// Reads and checks one extension directory.
#[cfg(test)]
pub fn load(dir: &Path) -> Result<Extension, String> {
    read(dir).map_err(|why| {
        Problem {
            dir: dir.to_owned(),
            why,
        }
        .to_string()
    })
}

fn read(dir: &Path) -> Result<Extension, String> {
    let text = std::fs::read_to_string(dir.join("extension.toml")).map_err(|e| e.to_string())?;
    let manifest: Manifest = toml::from_str(&text).map_err(|err| err.message().to_owned())?;
    check(&manifest, dir)?;
    Ok(Extension {
        dir: dir.to_owned(),
        manifest,
    })
}

fn check(m: &Manifest, dir: &Path) -> Result<(), String> {
    if m.contract != CONTRACT {
        return Err(format!(
            "contract {} is not supported; this rexboard speaks contract {CONTRACT}",
            m.contract
        ));
    }
    if !is_name(&m.name) {
        return Err(format!("name {:?} must be [a-z0-9-]+", m.name));
    }
    if dir.file_name().and_then(|n| n.to_str()) != Some(m.name.as_str()) {
        return Err(format!("name {:?} must match its directory's name", m.name));
    }
    if m.cmd.first().is_none_or(String::is_empty) {
        return Err("cmd must name a program".to_owned());
    }
    if m.every < 2 {
        return Err(format!("every is {}s; the least is 2", m.every));
    }
    if m.timeout == 0 || m.timeout >= m.every {
        return Err(format!(
            "timeout is {}s; it must be at least 1 and less than every ({}s)",
            m.timeout, m.every
        ));
    }
    if let Some(icon) = m.icon.as_deref().filter(|icon| !is_icon(icon)) {
        return Err(format!(
            "icon {icon:?} must be one character, one cell wide"
        ));
    }
    let mut seen = BTreeSet::new();
    for field in &m.fields {
        if !is_name(&field.name) {
            return Err(format!("field {:?} must be [a-z0-9-]+", field.name));
        }
        if let Some(icon) = field.icon.as_deref().filter(|icon| !is_icon(icon)) {
            return Err(format!(
                "field {:?}'s icon {icon:?} must be one character, one cell wide",
                field.name
            ));
        }
        if !seen.insert(&field.name) {
            return Err(format!("field {:?} is declared twice", field.name));
        }
    }
    let declared: BTreeSet<&str> = m.fields.iter().map(|f| f.name.as_str()).collect();
    let (mut names, mut keys) = (BTreeSet::new(), BTreeSet::new());
    for action in &m.actions {
        action.check(&declared)?;
        if !names.insert(&action.name) {
            return Err(format!("action {:?} is declared twice", action.name));
        }
        if !keys.insert(&action.key) {
            return Err(format!(
                "action {:?}: the key {:?} is already an action's",
                action.name, action.key
            ));
        }
    }
    Ok(())
}

/// One printable character one cell wide, as the card's label column takes it.
fn is_icon(icon: &str) -> bool {
    let mut chars = icon.chars();
    chars.next().is_some_and(|c| !c.is_control())
        && chars.next().is_none()
        && unicode_width::UnicodeWidthStr::width(icon) == 1
}

pub(crate) fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A field's id on a row.
pub fn field_id(extension: &str, field: &str) -> String {
    format!("{extension}.{field}")
}

/// What the board calls each field: its short name when no other enabled extension declares
/// the same one, else its full id.
pub fn field_names(enabled: &[&Extension]) -> BTreeMap<String, String> {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for ext in enabled {
        for field in ext.manifest.fields.iter().filter(|f| !f.hidden) {
            *count.entry(field.name.as_str()).or_default() += 1;
        }
    }
    let mut names = BTreeMap::new();
    for ext in enabled {
        for field in ext.manifest.fields.iter().filter(|f| !f.hidden) {
            let id = field_id(ext.name(), &field.name);
            let shown = if count[field.name.as_str()] == 1 {
                field.name.clone()
            } else {
                id.clone()
            };
            names.insert(id, shown);
        }
    }
    names
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn write(root: &Path, name: &str, manifest: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extension.toml"), manifest).unwrap();
        dir
    }

    pub fn manifest(name: &str, fields: &[&str]) -> String {
        let mut text = format!("name = \"{name}\"\ncontract = 1\ncmd = [\"./adapter\"]\n");
        for field in fields {
            text.push_str(&format!(
                "[[fields]]\nname = \"{field}\"\ntitle = \"{}\"\n",
                field.to_uppercase()
            ));
        }
        text
    }

    #[test]
    fn a_good_manifest_loads_with_its_defaults_and_a_local_adapter() {
        let root = tempfile::tempdir().unwrap();
        let dir = write(root.path(), "fake", &manifest("fake", &["rig"]));
        let ext = load(&dir).unwrap();
        assert_eq!((ext.manifest.every, ext.manifest.timeout), (10, 5));
        assert!(!ext.manifest.provides.status);
        assert_eq!(ext.argv(), [dir.join("adapter").display().to_string()]);
        assert!(ext.declares("rig") && !ext.declares("project"));
    }

    fn with_icon(name: &str, fields: &[&str], icon: &str) -> String {
        manifest(name, fields).replace(
            "contract = 1\n",
            &format!("contract = 1\nicon = \"{icon}\"\n"),
        )
    }

    #[test]
    fn an_extension_and_its_fields_may_each_name_a_one_cell_icon() {
        let root = tempfile::tempdir().unwrap();
        let text = with_icon("fake", &["rig"], "\u{ea7e}") + "icon = \"\u{f502}\"\n";
        let ext = load(&write(root.path(), "fake", &text)).unwrap();
        assert_eq!(ext.manifest.icon.as_deref(), Some("\u{ea7e}"));
        assert_eq!(ext.manifest.fields[0].icon.as_deref(), Some("\u{f502}"));
        let plain = load(&write(root.path(), "plain", &manifest("plain", &["rig"]))).unwrap();
        assert_eq!(plain.manifest.icon, None, "optional");
    }

    #[test]
    fn a_bad_manifest_says_why() {
        let root = tempfile::tempdir().unwrap();
        let cases = [
            (
                "two",
                manifest("two", &[]).replace("contract = 1", "contract = 2"),
                "contract 2 is not supported; this rexboard speaks contract 1",
            ),
            (
                "odd",
                manifest("odd", &[]) + "colour = \"red\"\n",
                "unknown field `colour`",
            ),
            (
                "dir",
                manifest("other", &[]),
                "must match its directory's name",
            ),
            ("Caps", manifest("Caps", &[]), "must be [a-z0-9-]+"),
            (
                "fast",
                manifest("fast", &[]) + "every = 1\n",
                "the least is 2",
            ),
            (
                "slow",
                manifest("slow", &[]) + "every = 3\ntimeout = 3\n",
                "less than every",
            ),
            (
                "twice",
                manifest("twice", &["rig", "rig"]),
                "declared twice",
            ),
            (
                "wide",
                with_icon("wide", &[], "中"),
                "icon \"中\" must be one character, one cell wide",
            ),
            ("pair", with_icon("pair", &[], "ab"), "one character"),
            ("ctl", with_icon("ctl", &[], "\\u001b"), "one character"),
            ("none", with_icon("none", &[], ""), "one character"),
            (
                "mark",
                manifest("mark", &["rig"]) + "icon = \"ab\"\n",
                "field \"rig\"'s icon \"ab\" must be one character",
            ),
        ];
        for (dir, text, why) in cases {
            let err = load(&write(root.path(), dir, &text)).unwrap_err();
            assert!(err.contains(why), "{dir}: {err}");
        }
    }

    #[test]
    fn discovery_skips_bad_manifests_and_keeps_the_first_of_a_name() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        write(one.path(), "fake", &manifest("fake", &["rig"]));
        write(one.path(), "broken", "name = ");
        write(two.path(), "fake", &manifest("fake", &["other"]));
        write(two.path(), "more", &manifest("more", &[]));
        let found = discover(&[one.path().to_owned(), two.path().to_owned()]);
        assert_eq!(found.found.keys().collect::<Vec<_>>(), ["fake", "more"]);
        assert!(found.found["fake"].declares("rig"), "the first root wins");
        assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
        let said: Vec<String> = found.problems.iter().map(Problem::to_string).collect();
        assert!(
            said.iter().any(|p| p.contains("broken/extension.toml")),
            "{said:?}"
        );
        assert!(said.iter().any(|p| p.contains("shadowed")), "{said:?}");
    }

    #[test]
    fn a_field_two_extensions_share_is_shown_by_its_full_id() {
        let root = tempfile::tempdir().unwrap();
        let a = load(&write(
            root.path(),
            "a",
            &manifest("a", &["project", "rig"]),
        ))
        .unwrap();
        let b = load(&write(root.path(), "b", &manifest("b", &["project"]))).unwrap();
        let names = field_names(&[&a, &b]);
        assert_eq!(names["a.project"], "a.project");
        assert_eq!(names["b.project"], "b.project");
        assert_eq!(names["a.rig"], "rig");
    }
}
