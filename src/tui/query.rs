//! The board's views, `/` filter, `f` status and `o` sort for the CLI: the rows `ls` prints and
//! `seen --all-done` marks, in the board's order, through the board's own code.

use std::collections::{BTreeMap, BTreeSet};

use tui_input::Input;

use super::app::{App, Item};
use super::columns::{COLUMNS, Dir, Sort, field_column};
use super::keep::Keep;
use super::status::{ALL, Status};
use super::views::{Key, View, value};
use crate::board::Row;
use crate::client::Field;
use crate::error::{Error, Result};

/// What to show, as `ls` takes it.
#[derive(Debug, Default)]
pub struct Query<'a> {
    pub view: Option<&'a str>,
    /// Each one narrows the rows as `/` does with one.
    pub filters: &'a [String],
    /// A status by its name or its `f` letter.
    pub status: Option<&'a str>,
    /// `COLUMN`, `COLUMN:asc` or `COLUMN:desc`.
    pub sort: Option<&'a str>,
    /// `GROUPING=GROUP`: only the rows in these groups, as the kept groups of the board do.
    pub keep: &'a [String],
    /// Every row the board has, when the rows given are only some of them (`--host`, `--marked`):
    /// a group's name, `API @MINI`, depends on the rows of the other hosts too.
    pub context: &'a [Row],
}

impl Query<'_> {
    pub fn is_empty(&self) -> bool {
        self.view.is_none()
            && self.filters.is_empty()
            && self.status.is_none()
            && self.sort.is_none()
            && self.keep.is_empty()
    }
}

/// `--keep GROUPING=GROUP` split into its two halves, `GROUPING` a view that groups rows.
fn split_keep(specs: &[String]) -> Result<Vec<(&str, &str)>> {
    specs
        .iter()
        .map(|spec| match spec.split_once('=') {
            Some((grouping, group)) if !grouping.is_empty() && !group.is_empty() => {
                Ok((grouping, group))
            }
            _ => Err(Error::usage_with(
                format!("--keep {spec}: write GROUPING=GROUP, as in --keep session=api@mini"),
                &["rexboard ls --view session --keep session=api@mini"],
            )),
        })
        .collect()
}

/// What each `--keep` names: the grouping's key, and the group as its header's name reads, in
/// lower case for a status or a harness, `api@mini` for a session.
fn kept_groups(app: &App, specs: &[String]) -> Result<Vec<(Key, String)>> {
    let views = app.views();
    split_keep(specs)?
        .into_iter()
        .map(|(grouping, group)| {
            let key = views
                .iter()
                .find(|view| view.name == grouping)
                .and_then(|view| view.group_by)
                .ok_or_else(|| {
                    let names: Vec<&str> = (views.iter())
                        .filter(|view| view.group_by.is_some())
                        .map(|view| view.name.as_str())
                        .collect();
                    Error::usage_with(
                        format!(
                            "--keep {grouping}={group}: no grouping named {grouping}; groupings: {}",
                            names.join(", ")
                        ),
                        &["rexboard views"],
                    )
                })?;
            Ok((key, group.to_owned()))
        })
        .collect()
}

/// Makes `kept` the board's kept groups, with only-kept on.
fn keep_only(app: &mut App, kept: &[(Key, String)]) {
    app.keep = Keep {
        only: true,
        ..Keep::default()
    };
    for (key, group) in kept {
        app.keep.add(&key.grouping(), group);
    }
}

/// `rows` as the board would list them under `query`: grouped and ordered by its view and sort,
/// and only those its filters and status keep.
pub fn arrange(rows: Vec<Row>, fields: Vec<Field>, query: &Query) -> Result<Vec<Row>> {
    let mut app = board(fields);
    // Not set_rows, which keeps only agents: `ls --all` asks for plain shells too.
    let wanted: BTreeSet<String> = rows.iter().map(|row| row.key.clone()).collect();
    app.rows = if query.context.is_empty() {
        rows
    } else {
        query.context.to_vec()
    };
    if let Some(name) = query.view {
        app.view = view(&app, name)?;
    }
    if let Some(status) = query.status {
        app.status = Some(status_of(status)?);
    }
    if let Some(spec) = query.sort {
        app.sort = Some(sort_of(&app, spec)?);
    }
    if !query.keep.is_empty() {
        let kept = kept_groups(&app, query.keep)?;
        for (key, group) in &kept {
            if !app
                .rows
                .iter()
                .any(|row| value(*key, row).label() == *group)
            {
                let hint = if *key == Key::Session {
                    " (SESSION@HOST)"
                } else {
                    ""
                };
                eprintln!(
                    "note: --keep {}={group}: no rows in that group{hint}",
                    key.grouping()
                );
            }
        }
        keep_only(&mut app, &kept);
    }
    let mut shown = listed(&app);
    shown.retain(|&i| wanted.contains(&app.rows[i].key));
    let groups = view_groups(&app);
    for filter in query.filters {
        app.filter = Input::new(filter.clone());
        // As the board's `/` does, a `key:` no row has is plain text, so `failed: 2` or a URL
        // searches the message; the note says so in case the key was a typo.
        if let Some(key) = app.unknown_key() {
            eprintln!(
                "note: --filter {filter}: no key named {key}, so it matches as text; {}",
                keys(&app)
            );
        }
        let kept: BTreeSet<usize> = listed(&app).into_iter().collect();
        shown.retain(|i| kept.contains(i));
    }
    Ok(shown
        .into_iter()
        .map(|i| Row {
            view_group: groups.get(&i).cloned(),
            ..app.rows[i].clone()
        })
        .collect())
}

/// Opens `app` as `rexboard board` and `pick` ask: the board's own state for `--status`, `--filter`
/// and `--view`, with the same values and `USAGE` errors as `ls`.
pub fn open(app: &mut App, query: &Query) -> Result<()> {
    if let Some(text) = query.status {
        app.status = Some(status_of(text)?);
    }
    if !query.filters.is_empty() {
        app.filter = Input::new(query.filters.join(" "));
    }
    let groupings: Vec<&str> = split_keep(query.keep)?
        .into_iter()
        .map(|(grouping, _)| grouping)
        .collect();
    // A field's view exists only once serve has said which extensions declare it; the built-in
    // views need no serve.
    if (query.view.iter().chain(&groupings))
        .any(|name| !app.views().iter().any(|v| v.name == *name))
    {
        app.set_extensions(crate::client::current()?.fields, BTreeMap::new());
    }
    if let Some(name) = query.view {
        app.show(view(app, name)?);
    }
    if !query.keep.is_empty() {
        keep_only(app, &kept_groups(app, query.keep)?);
    }
    Ok(())
}

/// Each row's group under a view that groups by something but status, which `group` has said
/// already, by the row's index: the header's name with no count.
fn view_groups(app: &App) -> BTreeMap<usize, String> {
    let mut groups = BTreeMap::new();
    if app.view.group_by.is_none_or(|key| key == Key::Status) {
        return groups;
    }
    let mut title = String::new();
    for item in app.items() {
        match item {
            Item::Header { bucket, .. } => title = app.group_title(&bucket),
            Item::Row(i) => {
                groups.insert(i, title.clone());
            }
            Item::Gap => {}
        }
    }
    groups
}

/// The first column's header for `ls --view NAME`: what the view groups by (`SESSION`, `HOST`,
/// `HARNESS`, a field's title), or None when it keeps the status `GROUP`.
pub fn heading(fields: Vec<Field>, name: &str) -> Option<String> {
    let app = board(fields);
    Some(match view(&app, name).ok()?.group_by? {
        Key::Status => return None,
        Key::Harness => "HARNESS".to_owned(),
        Key::Host => "HOST".to_owned(),
        Key::Session => "SESSION".to_owned(),
        Key::Field(id) => app
            .fields
            .iter()
            .find(|(field, _)| field.id == id)?
            .0
            .title
            .clone(),
    })
}

/// Each view `v` cycles through: its name, and what it groups by, or None for one list.
pub fn views(fields: Vec<Field>) -> Vec<(String, Option<String>)> {
    let app = board(fields);
    app.views()
        .into_iter()
        .map(|view| {
            let by = view.group_by.map(|key| match key {
                Key::Status => "status".to_owned(),
                Key::Harness => "harness".to_owned(),
                Key::Host => "host".to_owned(),
                Key::Session => "session".to_owned(),
                Key::Field(id) => id.to_owned(),
            });
            (view.name, by)
        })
        .collect()
}

fn board(fields: Vec<Field>) -> App {
    let mut app = App::default();
    app.set_extensions(fields, BTreeMap::new());
    app
}

/// The rows the board lists, by index, in order. No group is folded here.
fn listed(app: &App) -> Vec<usize> {
    app.items()
        .into_iter()
        .filter_map(|item| match item {
            Item::Row(i) => Some(i),
            Item::Header { .. } | Item::Gap => None,
        })
        .collect()
}

fn view(app: &App, name: &str) -> Result<View> {
    let all = app.views();
    all.iter()
        .find(|view| view.name == name)
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = all.iter().map(|view| view.name.as_str()).collect();
            Error::usage_with(
                format!("no view named {name}; views: {}", names.join(", ")),
                &["rexboard views"],
            )
        })
}

pub(crate) fn status_of(text: &str) -> Result<Status> {
    let mut letters = text.chars();
    let letter = match (letters.next(), letters.next()) {
        (Some(letter), None) => Status::from_letter(letter),
        _ => None,
    };
    letter
        .or_else(|| ALL.into_iter().find(|status| status.name() == text))
        .ok_or_else(|| {
            let names: Vec<&str> = ALL.iter().map(|status| status.name()).collect();
            Error::usage_with(
                format!(
                    "no status named {text}; statuses: {}, or their `f` letters (w d q p e u i *)",
                    names.join(", ")
                ),
                &["rexboard ls --status question"],
            )
        })
}

/// `COLUMN[:asc|:desc]` by a sortable column's header or a field's name; without a direction,
/// the one its first `o` press sorts in.
fn sort_of(app: &App, spec: &str) -> Result<Sort> {
    let (name, dir) = match spec.split_once(':') {
        Some((name, "asc")) => (name, Some(Dir::Up)),
        Some((name, "desc")) => (name, Some(Dir::Down)),
        Some(_) => {
            return Err(Error::usage_with(
                format!("--sort {spec}: the direction is asc or desc"),
                &["rexboard ls --sort age:desc"],
            ));
        }
        None => (spec, None),
    };
    let name = name.to_lowercase();
    let sortable: Vec<_> = COLUMNS
        .iter()
        .filter(|column| column.letter.is_some())
        .chain(
            app.fields
                .iter()
                .map(|(field, letter)| field_column(field, *letter)),
        )
        .collect();
    let column = sortable
        .iter()
        .copied()
        .find(|column| column.header.to_lowercase() == name)
        .or_else(|| {
            let (field, letter) = app
                .fields
                .iter()
                .find(|(f, _)| f.name == name || f.id == name)?;
            Some(field_column(field, *letter))
        })
        .ok_or_else(|| {
            let names: Vec<String> = sortable.iter().map(|c| c.header.to_lowercase()).collect();
            Error::usage_with(
                format!(
                    "no column named {name} to sort by; columns: {}",
                    names.join(", ")
                ),
                &["rexboard ls --sort age"],
            )
        })?;
    Ok((column.id, dir.unwrap_or(column.first)))
}

fn keys(app: &App) -> String {
    let mut keys = vec![
        "host", "state", "kind", "ask", "harness", "session", "window", "block", "mark",
    ];
    keys.extend(app.fields.iter().map(|(field, _)| field.name.as_str()));
    format!("keys: {}", keys.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::app;

    #[test]
    fn a_filter_keeps_what_the_boards_slash_keeps_unknown_keys_as_text() {
        let board = app();
        let fields: Vec<Field> = board.fields.iter().map(|(f, _)| f.clone()).collect();
        for text in [
            "seat-b msg",
            "host:mini",
            "codex",
            "nope:1",
            "failed: 2",
            "https://x.y/z",
        ] {
            let mut slash = app();
            slash.filter = Input::new(text.to_owned());
            let want: Vec<String> = listed(&slash)
                .into_iter()
                .map(|i| slash.rows[i].key.clone())
                .collect();
            let query = Query {
                filters: &[text.to_owned()],
                ..Query::default()
            };
            let got: Vec<String> = arrange(board.rows.clone(), fields.clone(), &query)
                .unwrap()
                .into_iter()
                .map(|row| row.key)
                .collect();
            assert_eq!(got, want, "{text}");
        }
    }

    #[test]
    fn keep_lists_the_rows_the_board_shows_for_those_kept_groups() {
        use crate::tui::app::testkit::{fleet, in_view, keep_in, rows_shown};
        let mut board = fleet();
        for key in ["studio/api-1", "mini/api-1"] {
            keep_in(&mut board, "session", key);
        }
        keep_in(&mut board, "host", "studio/api-1");
        board.keep.only = true;
        let rows = board.rows.clone();
        let specs = ["session=api@studio", "session=api@mini", "host=studio"].map(String::from);
        let query = Query {
            view: Some("session"),
            keep: &specs,
            ..Query::default()
        };
        let mut got: Vec<String> = arrange(rows.clone(), Vec::new(), &query)
            .unwrap()
            .into_iter()
            .map(|row| row.key)
            .collect();
        got.sort();
        in_view(&mut board, "session");
        assert_eq!(got, rows_shown(&board));
        assert_eq!(got, ["studio/api-1", "studio/api-2"]);
        let bad = ["session=api@studio".to_owned(), "nope=x".to_owned()];
        let query = Query {
            keep: &bad,
            ..Query::default()
        };
        assert!(arrange(rows, Vec::new(), &query).is_err());
    }

    #[test]
    fn the_note_for_a_key_nothing_has_lists_mark() {
        assert!(keys(&board(Vec::new())).contains("block, mark"));
    }
}
