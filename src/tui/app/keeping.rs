//! Kept groups: `+` keeps the selected row's group, `f+` shows only the kept ones, `:unkeep`
//! starts over. A filter beside `/` and `f`, which `esc` and `ff` never touch.

use super::*;
use crate::tui::views::value;

impl App {
    /// The grouping `name` as a `Key`, or None for a field no extension declares now.
    fn key_of(&self, grouping: &str) -> Option<Key> {
        match grouping {
            "status" => Some(Key::Status),
            "harness" => Some(Key::Harness),
            "host" => Some(Key::Host),
            "session" => Some(Key::Session),
            other => {
                let id = other.strip_prefix("field:")?;
                let (field, letter) = self.fields.iter().find(|(f, _)| f.id == id)?;
                Some(Key::Field(id_of(field_column(field, *letter))))
            }
        }
    }

    /// Whether the row's group is kept in every grouping that has kept groups, whether or not
    /// only-kept is on. A grouping whose field no extension declares now says nothing.
    pub(super) fn in_kept(&self, row: &Row) -> bool {
        self.keep.groups.iter().all(|(grouping, labels)| {
            self.key_of(grouping)
                .is_none_or(|key| labels.contains(&value(key, row).label()))
        })
    }

    /// The kept groups' part of the one predicate every list uses.
    pub(super) fn matches_kept(&self, row: &Row) -> bool {
        !self.keep.only || self.in_kept(row)
    }

    /// Whether only-kept is on and some group is kept in a grouping that resolves now, so it
    /// narrows the list.
    pub fn only_kept(&self) -> bool {
        self.keep.only && self.keep.groups.keys().any(|g| self.key_of(g).is_some())
    }

    /// Whether `bucket`, a group of the current view, is kept.
    pub fn is_kept(&self, bucket: &Bucket) -> bool {
        (self.view.group_by).is_some_and(|key| self.keep.has(&key.grouping(), &bucket.label()))
    }

    /// The rows the `/` text keeps that are in the kept groups, for `f`'s footer.
    pub fn kept_count(&self) -> usize {
        if self.keep.groups.is_empty() {
            return 0;
        }
        (self.rows.iter())
            .filter(|row| self.in_kept(row) && self.matches_text(row))
            .count()
    }

    /// The rows that need you outside the kept groups, whatever the `f` status and the `/` text:
    /// what only-kept hides that the board is for.
    pub fn needs_you_elsewhere(&self) -> usize {
        if !self.only_kept() {
            return 0;
        }
        (self.rows.iter())
            .filter(|row| row.group == Group::NeedsYou && !self.in_kept(row))
            .count()
    }

    /// Whether the kept groups, and not the `f` status or the `/` text, leave the list empty.
    pub fn kept_empties(&self) -> bool {
        self.only_kept()
            && self.rows.iter().any(|row| {
                self.status.is_none_or(|status| status.matches(row)) && self.matches_text(row)
            })
    }

    /// Each grouping with kept groups that resolves now, in the order of the views, with how many
    /// are kept, and whether the grouping is an extension field's.
    pub fn kept_groupings(&self) -> Vec<(String, usize, bool)> {
        let views = self.views();
        let mut named: Vec<(usize, String, usize, bool)> = self
            .keep
            .groups
            .iter()
            .filter(|(grouping, _)| self.key_of(grouping).is_some())
            .map(|(grouping, labels)| {
                let at = views
                    .iter()
                    .position(|view| view.group_by.map(Key::grouping).as_deref() == Some(grouping))
                    .unwrap_or(usize::MAX);
                (
                    at,
                    self.grouping_name(grouping),
                    labels.len(),
                    grouping.starts_with("field:"),
                )
            })
            .collect();
        named.sort();
        named
            .into_iter()
            .map(|(_, name, n, field)| (name, n, field))
            .collect()
    }

    /// What a grouping is called: its view's name, or a field's.
    fn grouping_name(&self, grouping: &str) -> String {
        match grouping.strip_prefix("field:") {
            Some(id) => (self.fields.iter())
                .find(|(f, _)| f.id == id)
                .map_or_else(|| id.rsplit('.').next().unwrap_or(id), |(f, _)| &f.name)
                .to_owned(),
            None => grouping.to_owned(),
        }
    }

    /// `+`: keeps the selected row's group in the view's grouping, or drops it.
    pub(super) fn keep_group(&mut self) {
        let Some(key) = self.view.group_by else {
            self.say("no groups in the table view · v picks a grouped view");
            return;
        };
        let bucket = match &self.selected {
            Some(Spot::Group(bucket)) => Some(bucket.clone()),
            _ => self.current().and_then(|row| bucket(&self.view, row)),
        };
        let Some(bucket) = bucket else {
            return;
        };
        let label = bucket.label();
        if self.keep.toggle(&key.grouping(), &label) {
            let then = if self.keep.only {
                "f+ shows all"
            } else {
                "f+ shows only kept"
            };
            self.say(format!("{label} kept · {then}"));
        } else {
            self.say(format!("{label} dropped"));
            if self.keep.groups.is_empty() {
                self.keep.only = false;
            }
        }
        self.keep_selection_visible();
    }

    /// `f+`: only the kept groups show, or all of them again.
    pub(super) fn toggle_only_kept(&mut self) {
        if !self.keep.only && self.keep.groups.is_empty() {
            self.say("nothing kept · + keeps the selected row's group");
            return;
        }
        self.keep.only = !self.keep.only;
        self.say(if self.keep.only {
            "only kept groups"
        } else {
            "all groups"
        });
    }

    /// `:unkeep`: drops every kept group, one that is not on the board included.
    pub(super) fn unkeep(&mut self) {
        let was = self.keep.only;
        let dropped = self.keep.clear();
        let groups = if dropped == 1 { "group" } else { "groups" };
        if dropped == 0 && !was {
            self.say("no kept groups");
        } else {
            self.say(format!("dropped {dropped} kept {groups}"));
        }
        self.keep_selection_visible();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Field;
    use crate::tui::app::testkit::*;

    #[test]
    fn plus_keeps_and_drops_the_selected_rows_group_in_each_grouped_view_and_is_refused_in_table() {
        for (view, grouping, label) in [
            ("status", "status", "needs you"),
            ("harness", "harness", "claude"),
            ("host", "host", "studio"),
            ("session", "session", "api@studio"),
        ] {
            let mut app = fleet();
            keep_in(&mut app, view, "studio/api-1");
            assert!(app.keep.has(grouping, label), "{view}: {:?}", app.keep);
            assert!(!app.keep.only, "{view}: keeping does not turn only-kept on");
            let said = format!("{label} kept · f+ shows only kept");
            assert_eq!(app.toast(), Some(said.as_str()), "{view}");
            press(&mut app, KeyCode::Char('+'));
            assert!(app.keep.groups.is_empty(), "{view}: + again drops it");
            let said = format!("{label} dropped");
            assert_eq!(app.toast(), Some(said.as_str()), "{view}");
        }
        let mut app = fleet();
        keep_in(&mut app, "table", "studio/api-1");
        assert!(app.keep.groups.is_empty());
        assert_eq!(
            app.toast(),
            Some("no groups in the table view · v picks a grouped view")
        );
    }

    #[test]
    fn plus_works_in_a_field_view_and_on_a_folded_header() {
        let mut app = fleet();
        let field = Field {
            id: "fake.rig".into(),
            name: "rig".into(),
            title: "Rig".into(),
            icon: None,
            extension_icon: None,
            width: None,
        };
        app.set_extensions(vec![field], Default::default());
        let mut rows = app.rows.clone();
        rows[0].ext.insert("fake.rig".into(), "alpha".into());
        app.set_rows(rows, app.hosts.clone());
        keep_in(&mut app, "rig", "studio/api-1");
        assert!(app.keep.has("field:fake.rig", "alpha"), "{:?}", app.keep);
        assert_eq!(app.kept_groupings(), [("rig".to_owned(), 1, true)]);
        press(&mut app, KeyCode::Char('+'));
        in_view(&mut app, "session");
        app.selected = Some(Spot::Row("mini/ops-1".into()));
        typed(&mut app, "zc");
        assert!(matches!(app.selected, Some(Spot::Group(_))));
        press(&mut app, KeyCode::Char('+'));
        assert!(
            app.keep.has("session", "ops@mini"),
            "a folded header keeps its group"
        );
        assert_eq!(app.toast(), Some("ops@mini kept · f+ shows only kept"));
    }

    #[test]
    fn a_kept_header_says_so_in_words_with_only_kept_on_or_off() {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/api-1");
        let text = render(&app, 80, 30);
        assert!(text.contains("+ API @STUDIO (2) ┄"), "{text}");
        assert!(text.contains("\nAPI @MINI (1) ┄"), "{text}");
        assert!(!text.contains("+ API @MINI"), "{text}");
        typed(&mut app, "f+");
        let text = render(&app, 80, 30);
        assert!(text.contains("+ API @STUDIO (2) ┄"), "{text}");
        assert!(!text.contains("WEB"), "{text}");
        typed(&mut app, "zM");
        let text = render(&app, 80, 30);
        assert!(
            text.contains("▸ + API @STUDIO (2) ┄"),
            "the fold mark comes first: {text}"
        );
    }

    #[test]
    fn the_text_and_a_status_narrow_inside_the_kept_groups_and_esc_ff_and_a_cleared_text_leave_them()
     {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/api-1");
        keep_in(&mut app, "session", "mini/api-1");
        typed(&mut app, "f+");
        let kept = app.keep.clone();
        typed(&mut app, "fw");
        assert_eq!(rows_shown(&app), ["mini/api-1", "studio/api-2"]);
        press(&mut app, KeyCode::Char('/'));
        typed(&mut app, "mini");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            rows_shown(&app),
            ["mini/api-1"],
            "a status and text inside the kept groups"
        );
        typed(&mut app, "ff");
        assert_eq!(app.status, None);
        assert_eq!(app.keep, kept, "ff");
        press(&mut app, KeyCode::Esc);
        assert!(app.filter.value().is_empty());
        assert_eq!(app.keep, kept, "esc");
        assert_eq!(
            rows_shown(&app),
            ["mini/api-1", "studio/api-1", "studio/api-2"]
        );
        press(&mut app, KeyCode::Char('/'));
        typed(&mut app, "zzz");
        key(&mut app, ctrl('u'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.keep, kept, "clearing the text");
        assert_eq!(rows_shown(&app).len(), 3);
    }

    #[test]
    fn digits_n_the_quote_and_seen_reach_only_the_rows_shown() {
        let mut app = fleet();
        let mut rows = app.rows.clone();
        rows.iter_mut()
            .find(|r| r.key == "mini/ops-1")
            .unwrap()
            .marked = true;
        rows.iter_mut()
            .find(|r| r.key == "studio/web-1")
            .unwrap()
            .marked = true;
        app.set_rows(rows, app.hosts.clone());
        keep_in(&mut app, "session", "studio/api-1");
        keep_in(&mut app, "session", "studio/web-1");
        typed(&mut app, "f+");
        render(&app, 100, 30);
        press(&mut app, KeyCode::Char('3'));
        assert_eq!(selected(&app), "studio/web-1", "row 3 of the three shown");
        press(&mut app, KeyCode::Char('4'));
        assert_eq!(app.toast(), Some("no row 4 · rows 1-3 on screen"));
        app.selected = Some(Spot::Row("studio/api-2".into()));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(
            selected(&app),
            "studio/api-1",
            "n skips mini/ops-1 and studio/docs-1"
        );
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(
            selected(&app),
            "studio/api-1",
            "the only one that needs you here"
        );
        let Some(Effect::Jump(row)) = press(&mut app, KeyCode::Char('\'')) else {
            panic!("' jumps")
        };
        assert_eq!(row.key, "studio/web-1", "the marked row in the kept groups");
        assert_eq!(
            seen_all(&mut app),
            None,
            "studio/docs-1 is done and unseen, but outside the kept groups"
        );
        assert_eq!(app.toast(), Some("no unseen done rows"));
    }

    #[test]
    fn n_says_when_the_rows_that_need_you_are_outside_the_kept_groups() {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/web-1");
        typed(&mut app, "f+");
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(
            app.toast(),
            Some("nobody needs you in the kept groups · f+ shows all")
        );
    }

    #[test]
    fn unkeep_drops_every_kept_group_one_not_on_the_board_included_and_says_how_many() {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/api-1");
        keep_in(&mut app, "host", "mini/api-1");
        app.keep.add("session", "gone@studio");
        typed(&mut app, "f+");
        typed(&mut app, ":unkeep");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.keep, Keep::default());
        assert_eq!(app.toast(), Some("dropped 3 kept groups"));
        typed(&mut app, ":unkeep");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.toast(), Some("no kept groups"));
        keep_in(&mut app, "host", "mini/api-1");
        typed(&mut app, ":unkeep");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.toast(), Some("dropped 1 kept group"));
    }

    #[test]
    fn a_kept_session_that_left_and_came_back_with_new_ids_shows_again() {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/api-1");
        typed(&mut app, "f+");
        let prefs = app.board_prefs();
        let mut next = App {
            remember: true,
            ..App::default()
        };
        next.restore_board(prefs);
        assert!(next.keep.only);
        let mut web = row("studio/web-9", "idle", "quiet", true);
        web.pane.session = "web".into();
        next.set_rows(vec![web], app.hosts.clone());
        assert!(
            rows_shown(&next).is_empty(),
            "no row of the kept session is on the board"
        );
        let mut back = row("studio/api-9", "working", "working", true);
        back.pane.session = "api".into();
        let mut other = row("mini/api-9", "working", "working", true);
        other.pane.session = "api".into();
        next.set_rows(vec![back, other], app.hosts.clone());
        assert_eq!(rows_shown(&next), ["studio/api-9"]);
        assert!(next.keep.has("session", "api@studio"), "kept all along");
    }

    #[test]
    fn dropping_the_last_kept_group_turns_only_kept_off_and_f_plus_says_what_it_did() {
        let mut app = fleet();
        keep_in(&mut app, "session", "studio/api-1");
        typed(&mut app, "f+");
        assert_eq!(app.toast(), Some("only kept groups"));
        assert_eq!(rows_shown(&app).len(), 2);
        press(&mut app, KeyCode::Char('+'));
        assert!(!app.keep.only && app.keep.groups.is_empty());
        assert_eq!(rows_shown(&app).len(), 7);
        keep_in(&mut app, "session", "studio/web-1");
        assert_eq!(
            rows_shown(&app).len(),
            7,
            "keeping a group does not narrow by itself"
        );
        typed(&mut app, "f+f+");
        assert_eq!(app.toast(), Some("all groups"));
        assert!(!app.keep.only);
    }

    #[test]
    fn a_grouping_no_extension_declares_now_does_not_hide_rows() {
        let mut app = fleet();
        app.keep.add("field:gone.rig", "alpha");
        app.keep.only = true;
        assert_eq!(rows_shown(&app).len(), 7);
        assert!(
            !app.only_kept(),
            "nothing is narrowed, so the top bar says nothing"
        );
        assert!(app.kept_groupings().is_empty());
    }
}
