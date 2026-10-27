//! The board's views, `/` filter, `f` status and `o` sort for the CLI: the rows `ls` prints and
//! `seen --all-done` marks, in the board's order, through the board's own code.

use std::collections::{BTreeMap, BTreeSet};

use tui_input::Input;

use super::app::{App, Item};
use super::columns::{COLUMNS, Dir, Sort, field_column};
use super::status::{ALL, Status};
use super::views::{Key, View};
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
}

impl Query<'_> {
    pub fn is_empty(&self) -> bool {
        self.view.is_none()
            && self.filters.is_empty()
            && self.status.is_none()
            && self.sort.is_none()
    }
}

/// `rows` as the board would list them under `query`: grouped and ordered by its view and sort,
/// and only those its filters and status keep.
pub fn arrange(rows: Vec<Row>, fields: Vec<Field>, query: &Query) -> Result<Vec<Row>> {
    let mut app = board(fields);
    // Not set_rows, which keeps only agents: `ls --all` asks for plain shells too.
    app.rows = rows;
    if let Some(name) = query.view {
        app.view = view(&app, name)?;
    }
    if let Some(status) = query.status {
        app.status = Some(status_of(status)?);
    }
    if let Some(spec) = query.sort {
        app.sort = Some(sort_of(&app, spec)?);
    }
    let mut shown = listed(&app);
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
    Ok(shown.into_iter().map(|i| app.rows[i].clone()).collect())
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

fn status_of(text: &str) -> Result<Status> {
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
                    "no status named {text}; statuses: {}, or their first letters",
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
    let mut keys = vec!["host", "state", "harness", "session", "window", "block"];
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
}
