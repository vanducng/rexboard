//! `[[mark]]` rules in config.toml: which rows the user marked (decision 18).

use crate::board::Row;

/// What a row's `marked_by` says when a rule in config.toml marked it.
pub const CONFIG: &str = "config";

#[derive(Debug, Clone, PartialEq)]
enum Key {
    Session,
    Window,
    Block,
    Host,
    /// An extension field, by its id (`<extension>.<field>`), as `ext` in `ls --json` names it.
    Field(String),
}

impl Key {
    fn of(name: &str) -> Option<Key> {
        Some(match name {
            "session" => Key::Session,
            "window" => Key::Window,
            "block" => Key::Block,
            "host" => Key::Host,
            _ => Key::Field(
                name.strip_prefix("field.")
                    .filter(|id| !id.is_empty())?
                    .into(),
            ),
        })
    }

    fn value<'a>(&self, row: &'a Row) -> Option<&'a str> {
        match self {
            Key::Session => Some(&row.pane.session),
            Key::Window => Some(&row.pane.tab),
            Key::Block => Some(&row.pane.pane),
            Key::Host => Some(&row.pane.host),
            Key::Field(id) => row.ext.get(id).map(String::as_str),
        }
    }
}

/// Every check of one rule must hold.
#[derive(Debug, Clone, PartialEq)]
struct Rule(Vec<(Key, String)>);

/// The rules in force, and what was wrong with the ones skipped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Marks {
    rules: Vec<Rule>,
    pub skipped: Vec<String>,
}

impl Marks {
    /// The rules in `[[mark]]`. A rule with no keys, an unknown key or a value that is not text
    /// is skipped, named in `skipped`, and the rest still apply.
    pub fn new(mark: Option<&toml::Value>) -> Marks {
        let mut marks = Marks::default();
        let Some(mark) = mark else { return marks };
        let Some(tables) = mark.as_array() else {
            marks
                .skipped
                .push("[[mark]]: not an array of tables".into());
            return marks;
        };
        for (n, table) in tables.iter().enumerate() {
            match table
                .as_table()
                .ok_or("not a table".to_owned())
                .and_then(rule)
            {
                Ok(rule) => marks.rules.push(rule),
                Err(why) => marks
                    .skipped
                    .push(format!("[[mark]] #{}: skipped, {why}", n + 1)),
            }
        }
        marks
    }

    fn marks(&self, row: &Row) -> bool {
        self.rules.iter().any(|Rule(checks)| {
            checks
                .iter()
                .all(|(key, pattern)| key.value(row).is_some_and(|text| glob(pattern, text)))
        })
    }

    /// Adds `config` to the sources of the rows a rule matches, ahead of the extensions the row
    /// already names (a to z), and marks them.
    pub fn apply(&self, rows: &mut [Row]) {
        for row in rows {
            row.marked_by.retain(|source| source != CONFIG);
            if self.marks(row) {
                row.marked_by.insert(0, CONFIG.into());
            }
            row.marked = !row.marked_by.is_empty();
        }
    }
}

fn rule(table: &toml::Table) -> Result<Rule, String> {
    let mut checks = Vec::new();
    for (name, value) in table {
        // `field.id = "x"` reads as a table `field` holding `id`; `"field.id" = "x"` as one key.
        let named: Vec<(String, &toml::Value)> = match (name.as_str(), value.as_table()) {
            ("field", Some(fields)) => fields
                .iter()
                .map(|(id, value)| (format!("field.{id}"), value))
                .collect(),
            _ => vec![(name.clone(), value)],
        };
        for (name, value) in named {
            let key = Key::of(&name).ok_or_else(|| format!("unknown key {name:?}"))?;
            let pattern = value
                .as_str()
                .ok_or_else(|| format!("{name:?} is not text"))?;
            checks.push((key, pattern.to_owned()));
        }
    }
    if checks.is_empty() {
        return Err("no keys".into());
    }
    Ok(Rule(checks))
}

/// Whether `text` is `pattern`, where `*` stands for any run of characters (none too) and `?`
/// for any one; everything else is itself, and the whole text must match.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (pattern, text): (Vec<char>, Vec<char>) =
        (pattern.chars().collect(), text.chars().collect());
    let (mut p, mut t) = (0, 0);
    // Where the last `*` was, and how much of the text it has taken so far.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(c) if *c == '?' || *c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((at, taken)) => {
                    star = Some((at, taken + 1));
                    (p, t) = (at + 1, taken + 1);
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(text: &str) -> Marks {
        let config: crate::config::Config = toml::from_str(text).unwrap();
        Marks::new(config.mark.as_ref())
    }

    fn row(key: &str) -> Row {
        let (host, block) = key.split_once('/').unwrap();
        serde_json::from_value(serde_json::json!({
            "key": key, "host": host, "session_id": "session:api", "session": "api",
            "window_id": "window:main", "tab": "main", "block_id": block, "pane": block,
            "title": "", "harness": "claude-code", "state": "working", "kind": null,
            "msg": null, "updated_at": null, "source": "native", "children": {},
            "since": "2026-01-01T00:00:00.000Z", "since_estimated": false,
            "turn_started": null, "last_turn": null, "seen": true, "stale": false,
            "suspect": false, "suspect_reason": null, "group": "working"
        }))
        .unwrap()
    }

    /// `mini/seat-b`, `local/claude`, `mini/codex` and `local/seat-a`, all in session `api`,
    /// window `main`; the first has the field `rig.team`.
    fn rows() -> Vec<Row> {
        let mut rows: Vec<Row> = ["mini/seat-b", "local/claude", "mini/codex", "local/seat-a"]
            .map(row)
            .into();
        rows[0].ext.insert("rig.team".into(), "ops".into());
        rows
    }

    fn marked(marks: &Marks) -> Vec<String> {
        let mut rows = rows();
        marks.apply(&mut rows);
        rows.iter()
            .filter(|row| row.marked)
            .map(|row| row.key.clone())
            .collect()
    }

    fn keys(text: &str) -> Vec<String> {
        marked(&marks(text))
    }

    #[test]
    fn globs_take_star_and_question_mark_and_the_whole_text() {
        for (pattern, text, want) in [
            ("api", "api", true),
            ("api", "api2", false),
            ("api*", "api-gateway", true),
            ("*gateway", "api-gateway", true),
            ("a*y", "api-gateway", true),
            ("a*z", "api-gateway", false),
            ("*", "", true),
            ("a?i", "api", true),
            ("a?i", "ai", false),
            ("*a*a*", "banana", true),
            ("*a*b", "aab", true),
            ("a*b*c", "axbxbxc", true),
            ("seat-?", "seat-a", true),
            ("", "", true),
            ("", "a", false),
            ("é?", "éx", true),
        ] {
            assert_eq!(glob(pattern, text), want, "{pattern} on {text}");
        }
    }

    #[test]
    fn a_rule_matches_exactly_or_by_glob_on_every_key_it_has() {
        assert_eq!(keys("[[mark]]\nblock = \"seat-a\""), ["local/seat-a"]);
        assert_eq!(
            keys("[[mark]]\nblock = \"seat-*\""),
            ["mini/seat-b", "local/seat-a"]
        );
        assert_eq!(
            keys("[[mark]]\nblock = \"codex\"\nhost = \"mini\""),
            ["mini/codex"]
        );
        assert_eq!(
            keys("[[mark]]\nhost = \"mi??\""),
            ["mini/seat-b", "mini/codex"]
        );
        assert_eq!(
            keys("[[mark]]\nsession = \"api\"\nwindow = \"ma*\"").len(),
            4
        );
        assert!(keys("[[mark]]\nsession = \"api\"\nhost = \"nowhere\"").is_empty());
        assert!(
            keys("[[mark]]\nsession = \"API\"").is_empty(),
            "case counts"
        );
    }

    #[test]
    fn several_rules_each_mark_and_a_field_rule_reads_the_rows_field() {
        let team = "[[mark]]\nfield.\"rig.team\" = \"op?\"";
        assert_eq!(keys(team), ["mini/seat-b"]);
        assert_eq!(
            keys("[[mark]]\n\"field.rig.team\" = \"ops\""),
            ["mini/seat-b"]
        );
        let two = format!("{team}\n[[mark]]\nblock = \"claude\"");
        assert_eq!(
            keys(&two),
            ["mini/seat-b", "local/claude"],
            "any rule marks"
        );
        assert!(keys("[[mark]]\nfield.\"rig.nothing\" = \"*\"").is_empty());
        assert!(keys("[[mark]]\nfield.\"rig.team\" = \"web\"\nhost = \"mini\"").is_empty());
    }

    #[test]
    fn a_bad_rule_is_skipped_and_named_and_the_rest_still_apply() {
        let marks = marks(
            "[[mark]]\nsesion = \"x\"\n[[mark]]\n[[mark]]\nhost = 3\n[[mark]]\nfield.\"\" = \"x\"\n[[mark]]\nhost = \"local\"",
        );
        assert_eq!(marks.skipped.len(), 4, "{:?}", marks.skipped);
        assert!(marks.skipped[0].contains("#1") && marks.skipped[0].contains("sesion"));
        assert!(marks.skipped[1].contains("#2") && marks.skipped[1].contains("no keys"));
        assert!(marks.skipped[2].contains("not text"));
        assert_eq!(marked(&marks), ["local/claude", "local/seat-a"]);
        let odd = Marks::new(Some(&toml::Value::String("x".into())));
        assert!(
            odd.rules.is_empty() && odd.skipped.len() == 1,
            "{:?}",
            odd.skipped
        );
        assert!(Marks::new(None).skipped.is_empty());
    }

    #[test]
    fn applying_puts_config_first_among_the_extensions_a_row_already_names() {
        let mut rows = rows();
        rows[1].marked_by = vec!["openrig".into(), "team".into()];
        rows[1].marked = true;
        rows[3].marked_by = vec!["team".into()];
        rows[3].marked = true;
        marks("[[mark]]\nblock = \"seat-a\"\n[[mark]]\nblock = \"claude\"").apply(&mut rows);
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.marked, r.marked_by.clone()))
            .collect();
        let s = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            got,
            [
                (false, vec![]),
                (true, s(&["config", "openrig", "team"])),
                (false, vec![]),
                (true, s(&["config", "team"])),
            ]
        );
        let mut again = rows.clone();
        marks("[[mark]]\nblock = \"seat-a\"\n[[mark]]\nblock = \"claude\"").apply(&mut again);
        assert_eq!(again, rows, "applying twice changes nothing");
        Marks::default().apply(&mut again);
        assert_eq!(
            again[1].marked_by,
            s(&["openrig", "team"]),
            "no rule: the extensions' marks stay"
        );
        assert!(!again[0].marked);
    }
}
