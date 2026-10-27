//! Views: a group-by key plus a sort. `status` is the board as serve orders it; `table`,
//! `harness` and `host` regroup the same rows.

use super::columns::{Sort, compare};
use super::theme;
use crate::board::{Group, Row};

/// What a view groups rows by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Status,
    Harness,
    Host,
    /// An extension field, by its id.
    Field(&'static str),
}

#[derive(Clone, PartialEq, Debug)]
pub struct View {
    pub name: String,
    /// None for one list with no group headers.
    pub group_by: Option<Key>,
}

/// The views `v` cycles through, in order. The first is the default.
pub fn views() -> Vec<View> {
    [
        ("status", Some(Key::Status)),
        ("table", None),
        ("harness", Some(Key::Harness)),
        ("host", Some(Key::Host)),
    ]
    .into_iter()
    .map(|(name, group_by)| View {
        name: name.to_owned(),
        group_by,
    })
    .collect()
}

impl Default for View {
    fn default() -> View {
        views().remove(0)
    }
}

/// A group in a view: a status group, or one value of the key (`claude`, `mini`, `?`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Bucket {
    Status(Group),
    Value(String),
}

impl Bucket {
    /// The group header's name: `NEEDS YOU`, `CLAUDE`, `MINI`.
    pub fn title(&self) -> String {
        match self {
            Bucket::Status(group) => theme::group_title(*group).to_owned(),
            Bucket::Value(value) => value.to_uppercase(),
        }
    }
}

/// The value a row groups under, `?` when it has none.
fn value(key: Key, row: &Row) -> Bucket {
    match key {
        Key::Status => Bucket::Status(row.group),
        Key::Harness => Bucket::Value(match row.pane.harness.as_deref() {
            Some(name) if !name.is_empty() => crate::harness::display(name).to_owned(),
            _ => "?".to_owned(),
        }),
        Key::Host => Bucket::Value(row.pane.host.clone()),
        Key::Field(id) => {
            Bucket::Value(row.ext.get(id).cloned().unwrap_or_else(|| NONE.to_owned()))
        }
    }
}

/// The group of rows a field view finds no value for.
pub const NONE: &str = "-";

/// The bucket `row` falls in under `view`, if the view has groups.
pub fn bucket(view: &View, row: &Row) -> Option<Bucket> {
    view.group_by.map(|key| value(key, row))
}

/// `rows` (in serve's order) as `view` shows them: each group with its row indices, in order.
/// Status groups keep serve's order; other values run a to z with `?` and `-` last. Inside a group the
/// sort applies, else serve's order stays; in NEEDS YOU blocked rows stay first.
pub fn arrange(rows: &[Row], view: &View, sort: Option<Sort>) -> Vec<(Option<Bucket>, Vec<usize>)> {
    let mut groups: Vec<(Option<Bucket>, Vec<usize>)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let bucket = bucket(view, row);
        match groups.iter_mut().find(|(b, _)| *b == bucket) {
            Some((_, members)) => members.push(i),
            None => groups.push((bucket, vec![i])),
        }
    }
    if view.group_by.is_some_and(|key| key != Key::Status) {
        groups.sort_by(|(a, _), (b, _)| {
            let last =
                |b: &Option<Bucket>| matches!(b, Some(Bucket::Value(v)) if v == "?" || v == NONE);
            last(a).cmp(&last(b)).then_with(|| a.cmp(b))
        });
    }
    if let Some(sort) = sort {
        for (bucket, members) in &mut groups {
            let blocked_first = *bucket == Some(Bucket::Status(Group::NeedsYou));
            members.sort_by(|&a, &b| {
                let (a, b) = (&rows[a], &rows[b]);
                let unblocked = |row: &Row| blocked_first && row.pane.state != "blocked";
                unblocked(a)
                    .cmp(&unblocked(b))
                    .then_with(|| compare(a, b, sort))
            });
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::tui::app::testkit::{app, hosts, press, row, sort};
    use crate::tui::app::{App, Spot};
    use crate::tui::columns::{Col, Dir};

    fn named(view: &str) -> View {
        views().into_iter().find(|v| v.name == view).unwrap()
    }

    fn keys(rows: &[Row], groups: &[(Option<Bucket>, Vec<usize>)]) -> Vec<String> {
        groups
            .iter()
            .flat_map(|(_, members)| members.iter().map(|&i| rows[i].key.clone()))
            .collect()
    }

    /// Three states, two hosts, two harnesses and one row with no harness.
    fn mixed() -> Vec<Row> {
        let mut rows = Vec::new();
        for (key, state, group, since, harness) in [
            (
                "mini/old",
                "blocked",
                "needs_you",
                "2026-01-01T00:00:00Z",
                Some("codex"),
            ),
            (
                "local/new",
                "blocked",
                "needs_you",
                "2026-01-01T05:00:00Z",
                Some("claude-code"),
            ),
            (
                "local/fin",
                "done",
                "needs_you",
                "2026-01-01T01:00:00Z",
                Some("claude-code"),
            ),
            (
                "mini/run",
                "working",
                "working",
                "2026-01-01T03:00:00Z",
                None,
            ),
            (
                "local/rest",
                "idle",
                "quiet",
                "2026-01-01T02:00:00Z",
                Some("codex"),
            ),
        ] {
            let mut r = row(key, state, group, false);
            r.since = since.into();
            r.pane.harness = harness.map(str::to_owned);
            rows.push(r);
        }
        rows
    }

    #[test]
    fn status_keeps_serves_order_and_groups() {
        let rows = mixed();
        let groups = arrange(&rows, &named("status"), None);
        let buckets: Vec<_> = groups.iter().map(|(b, _)| b.clone()).collect();
        assert_eq!(
            buckets,
            [Group::NeedsYou, Group::Working, Group::Quiet].map(|g| Some(Bucket::Status(g)))
        );
        assert_eq!(
            keys(&rows, &groups),
            [
                "mini/old",
                "local/new",
                "local/fin",
                "mini/run",
                "local/rest"
            ]
        );
    }

    #[test]
    fn flat_sorted_by_age_goes_down_flips_up_and_clears() {
        let rows = mixed();
        let flat = named("table");
        let down = arrange(&rows, &flat, Some((Col::Age, Dir::Down)));
        assert_eq!(down.len(), 1);
        assert_eq!(down[0].0, None, "flat has no group header");
        assert_eq!(
            keys(&rows, &down),
            [
                "mini/old",
                "local/fin",
                "local/rest",
                "mini/run",
                "local/new"
            ]
        );
        let up = arrange(&rows, &flat, Some((Col::Age, Dir::Up)));
        assert_eq!(
            keys(&rows, &up),
            [
                "local/new",
                "mini/run",
                "local/rest",
                "local/fin",
                "mini/old"
            ]
        );
        let cleared = arrange(&rows, &flat, None);
        assert_eq!(
            keys(&rows, &cleared),
            keys(&rows, &arrange(&rows, &named("status"), None))
        );
    }

    #[test]
    fn harness_groups_run_a_to_z_with_unknown_last() {
        let rows = mixed();
        let groups = arrange(&rows, &named("harness"), None);
        let buckets: Vec<_> = groups.iter().map(|(b, _)| b.clone()).collect();
        assert_eq!(
            buckets,
            ["claude", "codex", "?"].map(|v| Some(Bucket::Value(v.to_owned())))
        );
        assert_eq!(
            keys(&rows, &groups),
            [
                "local/new",
                "local/fin",
                "mini/old",
                "local/rest",
                "mini/run"
            ]
        );
    }

    #[test]
    fn host_groups_sort_inside_each_group() {
        let rows = mixed();
        let groups = arrange(&rows, &named("host"), Some((Col::Age, Dir::Up)));
        assert_eq!(groups[0].0, Some(Bucket::Value("local".to_owned())));
        assert_eq!(
            keys(&rows, &groups),
            [
                "local/new",
                "local/rest",
                "local/fin",
                "mini/run",
                "mini/old"
            ]
        );
    }

    #[test]
    fn a_sort_in_the_status_view_keeps_blocked_first_in_needs_you() {
        let rows = mixed();
        let groups = arrange(&rows, &named("status"), Some((Col::Age, Dir::Up)));
        assert_eq!(
            groups[0]
                .1
                .iter()
                .map(|&i| rows[i].key.as_str())
                .collect::<Vec<_>>(),
            ["local/new", "mini/old", "local/fin"],
            "the newest blocked row first, and done after both blocked rows"
        );
    }

    #[test]
    fn a_row_with_no_age_sorts_last_either_way() {
        let mut rows = mixed();
        rows[1].pane.source = "inferred".into();
        let flat = named("table");
        for dir in [Dir::Up, Dir::Down] {
            let order = keys(&rows, &arrange(&rows, &flat, Some((Col::Age, dir))));
            assert_eq!(order.last().unwrap(), "local/new", "{dir:?}");
        }
    }

    fn type_in(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn v_cycles_the_views_and_comes_back_to_status() {
        let mut app = app();
        let mut seen = vec![app.view.name.clone()];
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('v'));
            seen.push(app.view.name.clone());
        }
        assert_eq!(seen, ["status", "table", "harness", "host", "status"]);
    }

    #[test]
    fn the_selected_row_keeps_its_key_across_views_and_sorts() {
        let mut app = App::default();
        app.set_rows(mixed(), hosts());
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        let picked = app.selected.clone();
        assert_eq!(picked, Some(Spot::Row("local/fin".into())));
        for keys in ["v", "oa", "oa", "v", "os", "v"] {
            for key in keys.chars() {
                press(&mut app, KeyCode::Char(key));
            }
            assert_eq!(app.selected, picked, "after {keys}");
        }
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.selected, picked, "after esc");
    }

    #[test]
    fn o_letters_sort_flip_and_esc_clears() {
        let mut app = app();
        sort(&mut app, 'a');
        assert_eq!(
            app.sort,
            Some((Col::Age, Dir::Down)),
            "the longest wait first"
        );
        sort(&mut app, 'a');
        assert_eq!(app.sort, Some((Col::Age, Dir::Up)), "the same letter flips");
        sort(&mut app, 'h');
        assert_eq!(
            app.sort,
            Some((Col::Harness, Dir::Up)),
            "a new letter starts over"
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.sort, None);
    }

    #[test]
    fn every_sort_letter_sorts_then_flips_then_clears() {
        use crate::tui::columns::COLUMNS;
        let mut app = app();
        app.rows[0].usage = Some(crate::harness::usage::Usage {
            ctx_pct: Some(7),
            ..Default::default()
        });
        sort(&mut app, 'c');
        assert_eq!(app.sort, None, "COST sorts only once tui.json asks for it");
        app.columns = vec!["tok".into(), "cost".into(), "5h".into()];
        for column in COLUMNS.iter().filter(|c| c.letter.is_some()) {
            let letter = column.letter.unwrap();
            let press_it = |app: &mut App| sort(app, letter);
            press_it(&mut app);
            assert_eq!(app.sort, Some((column.id, column.first)), "{letter} once");
            press_it(&mut app);
            assert_eq!(
                app.sort,
                Some((column.id, column.first.flip())),
                "{letter} twice"
            );
            press_it(&mut app);
            assert_eq!(app.sort, None, "{letter} a third time clears");
        }
    }

    #[test]
    fn the_colon_prompt_completes_and_names_a_missing_view() {
        let mut app = app();
        press(&mut app, KeyCode::Char(':'));
        type_in(&mut app, "ta");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.command.value(), "table");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.view.name, "table");
        assert!(!app.commanding);
        press(&mut app, KeyCode::Char(':'));
        type_in(&mut app, "h");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.command.value(), "h", "harness and host share only h");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.view.name, "table", "esc cancels");
        press(&mut app, KeyCode::Char(':'));
        type_in(&mut app, "nope");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.toast(), Some("no view \"nope\""));
        assert_eq!(app.view.name, "table");
    }

    #[test]
    fn flat_still_opens_the_table_view() {
        let mut app = app();
        press(&mut app, KeyCode::Char(':'));
        type_in(&mut app, "flat");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.view.name, "table");
    }

    #[test]
    fn digits_count_needs_you_rows_in_the_order_shown() {
        let mut app = App::default();
        app.set_rows(mixed(), hosts());
        sort(&mut app, 'a');
        sort(&mut app, 'a');
        assert_eq!(app.sort, Some((Col::Age, Dir::Up)));
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(
            app.selected,
            Some(Spot::Row("local/new".into())),
            "the first row drawn"
        );
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.selected, Some(Spot::Row("mini/old".into())));
    }

    #[test]
    fn coming_back_to_a_view_with_the_rows_group_folded_rests_on_its_header() {
        let mut app = App::default();
        app.set_rows(mixed(), hosts());
        // In status, fold WORKING and move up to a NEEDS YOU row.
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.selected, Some(Spot::Row("mini/run".into())));
        press(&mut app, KeyCode::Char('z'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('k'));
        // In table, select the working row, then come back to status.
        press(&mut app, KeyCode::Char('v'));
        while app.selected != Some(Spot::Row("mini/run".into())) {
            press(&mut app, KeyCode::Char('j'));
        }
        for _ in 0..3 {
            press(&mut app, KeyCode::Char('v'));
        }
        assert_eq!(app.view.name, "status");
        assert_eq!(
            app.selected,
            Some(Spot::Group(Bucket::Status(Group::Working))),
            "the folded group's header, not the first row"
        );
    }
}
