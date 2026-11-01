//! Views, the `:` line, sorting and the extensions' fields: how the list is grouped and ordered.

use super::*;
use crate::tui::columns::{Tier, width_set};

impl App {
    /// The views `v` cycles through: the built-in ones, then one per field, by its name, or by
    /// its id when a built-in view has that name.
    pub fn views(&self) -> Vec<View> {
        let mut all = views();
        for (field, letter) in &self.fields {
            let name = if all.iter().any(|v| v.name == field.name) {
                field.id.clone()
            } else {
                field.name.clone()
            };
            let id = id_of(field_column(field, *letter));
            all.push(View {
                name,
                group_by: Some(Key::Field(id)),
            });
        }
        all
    }

    /// Switches to `view`; the selection keeps its row.
    pub(super) fn show(&mut self, view: View) {
        if let Some(Spot::Group(_)) = self.selected {
            self.unfold_all();
        }
        self.view = view;
        if self.clicks() && !self.told_table {
            self.told_table = true;
            self.say("click a header to sort · hold Option (iTerm2) or Shift to select text");
        }
        // A row in a group this view keeps folded rests on that group's header, as `z` does.
        if let Some(group) = self.current().and_then(|row| bucket(&self.view, row))
            && self.folded.contains(&group)
        {
            self.selected = Some(Spot::Group(group));
        }
        self.keep_selection_visible();
    }

    /// The `:` prompt's Tab: the one view name the typed text starts, else the longest prefix
    /// the names share.
    pub(super) fn complete(&mut self) {
        let typed = self.command.value().to_owned();
        let names: Vec<String> = self
            .views()
            .into_iter()
            .map(|v| v.name)
            .filter(|name| name.starts_with(&typed))
            .collect();
        let Some(first) = names.first() else {
            return;
        };
        let common = names.iter().fold(first.clone(), |acc, name| {
            acc.chars()
                .zip(name.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect()
        });
        self.command = Input::new(common);
    }

    /// The `:` prompt's Enter: `seen` marks every done row shown seen, `q` quits, else a view's
    /// name, or the start of exactly one.
    pub(super) fn open(&mut self) -> Option<Effect> {
        match self.command.value().trim() {
            "seen" => return self.seen_all(),
            "q" | "q!" | "quit" => return Some(Effect::Quit),
            _ => {}
        }
        let typed = match self.command.value().trim() {
            // `flat` was the table view's name before v0.3.
            "flat" => "table".to_owned(),
            typed => typed.to_owned(),
        };
        let all = self.views();
        let found = all.iter().find(|v| v.name == typed).cloned().or_else(|| {
            let mut starts = all.iter().filter(|v| v.name.starts_with(&typed));
            match (starts.next(), starts.next()) {
                (Some(one), None) if !typed.is_empty() => Some(one.clone()),
                _ => None,
            }
        });
        match found {
            Some(view) => self.show(view),
            None => self.say(format!("no view \"{typed}\"")),
        }
        None
    }

    /// `o` and a column's letter: sort by it, flip it if it is the sort, clear it the third time.
    pub(super) fn sort_by(&mut self, letter: char) {
        let Some(column) = self.column_by_letter(letter) else {
            return;
        };
        self.sort = match self.sort {
            Some((id, dir)) if id == column.id && dir == column.first => Some((id, dir.flip())),
            Some((id, _)) if id == column.id => None,
            _ => Some((column.id, column.first)),
        };
    }

    /// The column a sort letter names: a built-in one, or a field's.
    pub(super) fn column_by_letter(&self, letter: char) -> Option<&'static Column> {
        self.builtin_columns()
            .find(|c| c.letter == Some(letter))
            .or_else(|| {
                let (field, _) = self.fields.iter().find(|(_, l)| *l == Some(letter))?;
                Some(field_column(field, Some(letter)))
            })
    }

    /// The sort letters `o` takes, built-in then the fields'.
    pub fn sort_letters(&self) -> Vec<String> {
        self.builtin_columns()
            .filter_map(|c| c.letter)
            .chain(self.fields.iter().filter_map(|(_, l)| *l))
            .map(String::from)
            .collect()
    }

    /// The built-in columns `o` can sort by: the usage ones only while they show.
    pub fn builtin_columns(&self) -> impl Iterator<Item = &'static Column> {
        let usage = self.usage_columns().cols;
        crate::tui::columns::COLUMNS
            .iter()
            .filter(move |c| !crate::tui::columns::is_usage(c.id) || usage.contains(&c.id))
    }

    /// The usage columns the list shows: none until some row reports what its turn used, then
    /// MODEL and CTX, and TOK, COST and 5H when tui.json's `columns` names them.
    pub fn usage_columns(&self) -> UsageShown {
        use crate::tui::columns::{USAGE, USAGE_SHOWN, opted};
        let models: Vec<&str> = self
            .rows
            .iter()
            .filter_map(|row| row.usage.as_ref())
            .map(|usage| usage.model.as_deref().unwrap_or_default())
            .collect();
        if models.is_empty() {
            return UsageShown::default();
        }
        UsageShown {
            cols: USAGE
                .into_iter()
                .filter(|c| USAGE_SHOWN.contains(c) || opted(&self.columns, *c))
                .collect(),
            model: models.iter().map(|m| m.width()).max().unwrap_or_default(),
        }
    }

    /// The list's columns on a board `board` wide, `list` of it for the list: a view grouped by
    /// harness or host leaves that column to its group headers, and nerd marks widen HARNESS.
    pub fn columns(&self, board: u16, list: usize) -> Columns {
        let mut columns =
            Columns::with_usage(board, list, &self.field_columns(), &self.usage_columns());
        match self.view.group_by {
            Some(Key::Harness) => columns.resize(Col::Harness, 0),
            Some(Key::Host) => columns.resize(Col::Host, 0),
            _ => {}
        }
        if self.nerd {
            columns.marks = true;
            columns.resize(Col::Harness, columns.width(Col::Harness) + 2);
        }
        let widest = |text: &dyn Fn(&Row) -> String| {
            self.rows
                .iter()
                .map(|row| text(row).width())
                .max()
                .unwrap_or_default()
        };
        columns.fit(
            board,
            &Longest {
                host: widest(&|row| clean(&row.pane.host)),
                msg: widest(&|row| markdown::text(&markdown::msg(row))),
                // The block's name has room for a twin's ` #N`.
                parts: [
                    widest(&|row| clean(&row.pane.session)).min(LABEL_MAX),
                    widest(&|row| clean(&row.pane.tab)).min(LABEL_MAX),
                    widest(&crate::output::name).min(LABEL_MAX) + 3,
                ],
                place: widest(&|row| path(&labels(row, columns.tier != Tier::Wide), usize::MAX))
                    + 3,
                fields: self
                    .fields
                    .iter()
                    .map(|(f, letter)| {
                        let id = id_of(field_column(f, *letter));
                        // `extension.field` beats the bare name, and both beat the manifest; a
                        // bare name that is a built-in column's is that column's.
                        let user = width_set(&self.widths, &f.id).or_else(|| {
                            let builtin = ["host", "session", "window", "block", "msg"];
                            width_set(&self.widths, &f.name)
                                .filter(|_| !builtin.contains(&f.name.as_str()))
                        });
                        FieldFit {
                            id: Col::Field(id),
                            longest: widest(&|row| row.ext.get(id).cloned().unwrap_or_default()),
                            fixed: user.or(f.width),
                            yours: user.is_some(),
                        }
                    })
                    .collect(),
            },
            &self.widths,
        );
        columns
    }

    /// The columns of the fields, but for the one the view groups by, whose group header says it.
    pub fn field_columns(&self) -> Vec<&'static Column> {
        self.fields
            .iter()
            .map(|(f, letter)| field_column(f, *letter))
            .filter(|c| {
                self.view
                    .group_by
                    .is_none_or(|key| key != Key::Field(id_of(c)))
            })
            .collect()
    }

    /// New fields and runs from serve. A view or sort on a field that is gone goes back to the
    /// default.
    pub fn set_extensions(&mut self, fields: Vec<Field>, runs: BTreeMap<String, ExtRun>) {
        self.ext_runs = runs;
        if self.fields.iter().map(|(f, _)| f).eq(&fields) {
            return;
        }
        let mut taken = Vec::new();
        self.fields = fields
            .into_iter()
            .map(|field| {
                let letter = free_letter(&field.name, &taken);
                taken.extend(letter);
                (field, letter)
            })
            .collect();
        if !self.views().contains(&self.view) {
            self.view = View::default();
        }
        if let Some((Col::Field(id), _)) = self.sort
            && !self.fields.iter().any(|(f, _)| f.id == id)
        {
            self.sort = None;
        }
    }

    /// `ctrl+s`: every unseen `done` row the list shows, to be marked seen.
    pub(super) fn seen_all(&mut self) -> Option<Effect> {
        let keys: Vec<String> = self
            .rows
            .iter()
            .filter(|row| row.pane.state == "done" && !row.seen && self.matches(row))
            .map(|row| row.key.clone())
            .collect();
        if keys.is_empty() {
            self.say("no unseen done rows");
            return None;
        }
        Some(Effect::SeenAll(keys))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;
    use crate::tui::columns::Dir;

    #[test]
    fn colon_seen_marks_only_unseen_done_rows_and_o_s_sorts() {
        let mut app = App::default();
        let mut all = rows();
        all.push(row("mini/old-done", "done", "needs_you", false));
        all.push(row("mini/seen-done", "done", "quiet", true));
        all.push(row("mini/broken", "error", "needs_you", false));
        app.set_rows(all, hosts());
        assert_eq!(typed(&mut app, "os").pop().flatten(), None, "o s sorts");
        assert_eq!(app.sort, Some((Col::State, Dir::Up)));
        assert_eq!(
            seen_all(&mut app),
            Some(Effect::SeenAll(vec![
                "local/claude".into(),
                "mini/old-done".into()
            ])),
            "never blocked, error or already seen rows"
        );
        press(&mut app, KeyCode::Char('/'));
        for c in "old".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            seen_all(&mut app),
            Some(Effect::SeenAll(vec!["mini/old-done".into()])),
            "a kept filter narrows it to the rows shown"
        );
        press(&mut app, KeyCode::Esc);
        app.set_rows(
            vec![row("mini/seat-b", "blocked", "needs_you", false)],
            hosts(),
        );
        assert_eq!(seen_all(&mut app), None);
        assert_eq!(app.toast(), Some("no unseen done rows"));
    }
}
