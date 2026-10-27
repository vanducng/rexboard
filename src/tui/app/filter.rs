//! The `/` text and the `f` status: which rows the list keeps.

use super::*;

impl App {
    /// Whether the row passes the status filter and the `/` text.
    pub(super) fn matches(&self, row: &Row) -> bool {
        self.status.is_none_or(|status| status.matches(row)) && self.matches_text(row)
    }

    /// Rows the `/` text keeps, with any status; the counts `f` shows.
    pub fn status_count(&self, status: Status) -> usize {
        self.rows
            .iter()
            .filter(|row| status.matches(row) && self.matches_text(row))
            .count()
    }

    /// The `/` text: `key:value` terms match one key each, and the rest matches any label as
    /// one phrase, all ignoring case and all needing to match.
    pub(super) fn matches_text(&self, row: &Row) -> bool {
        let query = self.filter.value().to_lowercase();
        let mut free = Vec::new();
        for term in query.split_whitespace() {
            let keyed = term.split_once(':').and_then(|(key, want)| {
                let value = self.value_of(row, key)?;
                Some(value.is_some_and(|v| v.to_lowercase().contains(want)))
            });
            match keyed {
                Some(true) => {}
                Some(false) => return false,
                None => free.push(term),
            }
        }
        let free = free.join(" ");
        if free.is_empty() {
            return true;
        }
        let p = &row.pane;
        let shown = &self.marked(row).plain;
        [&p.host, &p.session, &p.tab, &p.pane, &p.title, &p.state]
            .into_iter()
            .chain(p.msg.as_ref())
            .chain(Some(shown))
            .chain(p.harness.as_ref())
            .any(|field| field.to_lowercase().contains(&free))
    }

    /// The first `key:` in the `/` text that names no key, for saying so when nothing matches.
    pub fn unknown_key(&self) -> Option<String> {
        let probe = self.rows.first()?;
        self.filter
            .value()
            .to_lowercase()
            .split_whitespace()
            .filter_map(|term| term.split_once(':'))
            .find(|(key, _)| !key.is_empty() && self.value_of(probe, key).is_none())
            .map(|(key, _)| key.to_owned())
    }

    /// What filter key `key` reads on a row: None when no key has that name, Some(None) when
    /// the row has no value for it.
    pub(super) fn value_of<'a>(&self, row: &'a Row, key: &str) -> Option<Option<&'a str>> {
        let p = &row.pane;
        Some(Some(match key {
            "host" => &p.host,
            "state" => &p.state,
            "harness" => return Some(p.harness.as_deref()),
            "session" => &p.session,
            "tab" | "window" => &p.tab,
            "pane" | "block" => &p.pane,
            _ => {
                let (field, _) = self
                    .fields
                    .iter()
                    .find(|(f, _)| f.name == key || f.id == key)?;
                return Some(row.ext.get(&field.id).map(String::as_str));
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::testkit::*;

    #[test]
    fn filter_narrows_across_groups() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        for c in "mini".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let header = |group, count| Item::Header {
            bucket: Bucket::Status(group),
            count,
            folded: false,
        };
        assert_eq!(
            app.items(),
            [
                header(Group::NeedsYou, 1),
                Item::Row(0),
                header(Group::Working, 1),
                Item::Row(2)
            ]
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.items().len(), 7, "esc clears the filter");
    }
}
