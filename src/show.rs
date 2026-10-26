//! `rexboard show` and `rexboard views`: what the board's card and `v` show, for agents.

use chrono::Utc;
use serde_json::json;

use crate::board::Row;
use crate::client::{self, Field};
use crate::error::{Error, Result};
use crate::output::{BREAK, clean, harness, labels, path, span, state_word};

/// The most `--tail` reads, as the card does.
const TAIL_MOST: usize = 200;

pub fn show(key: &str, tail: Option<usize>, json: bool) -> Result<()> {
    if !key.contains('/') {
        return Err(Error::usage_with(
            format!("KEY must be HOST/BLOCK_ID, as in ls --json: {key}"),
            &["rexboard ls --json"],
        ));
    }
    let rows = client::current()?;
    let row = rows
        .rows
        .into_iter()
        .find(|row| row.key == key)
        .ok_or_else(|| Error::NotFound(format!("no pane {key}")))?;
    let tail = tail
        .map(|lines| crate::tui::tail(&row, lines.min(TAIL_MOST)))
        .transpose()?;
    if json {
        let mut shown = serde_json::to_value(&row).map_err(std::io::Error::other)?;
        shown["where"] = json!(place(&row));
        if let Some(tail) = &tail {
            shown["tail"] = json!(tail);
        }
        println!("{shown}");
    } else {
        print!("{}", text(&row, &rows.fields, tail.as_deref()));
    }
    Ok(())
}

/// The card as plain text: who and how it stands, the message, then its facts and the tail.
fn text(row: &Row, fields: &[Field], tail: Option<&[String]>) -> String {
    let mut state = state_word(row);
    if let Some(kind) = &row.pane.kind {
        state = format!("{state} · {kind}");
    }
    let mut head = vec![crate::output::name(row), state];
    head.extend(crate::tui::card::outcome(row, Utc::now()));
    let mut out = format!("{}\n", clean(&head.join(" · ")));
    let msg = clean(row.pane.msg.as_deref().unwrap_or_default());
    for line in msg.split(BREAK).map(str::trim).filter(|l| !l.is_empty()) {
        out.push_str(&format!("{line}\n"));
    }
    let mut facts = vec![("host", row.pane.host.clone()), ("harness", harness(row))];
    facts.extend(row.last_turn.map(|secs| ("turn", span(secs))));
    facts.push(("where", place(row)));
    for field in fields {
        if let Some(value) = row.ext.get(&field.id) {
            facts.push((field.name.as_str(), clean(value)));
        }
    }
    let title = crate::pane::bare_title(&row.pane.title);
    if !title.is_empty() {
        facts.push(("title", clean(title)));
    }
    facts.push(("source", row.pane.source.clone()));
    facts.push(("key", row.key.clone()));
    let width = facts
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    out.push('\n');
    for (label, value) in facts {
        out.push_str(&format!("{label:<width$}  {value}\n"));
    }
    if let Some(tail) = tail {
        out.push('\n');
        for line in tail {
            out.push_str(&format!("{line}\n"));
        }
    }
    out
}

fn place(row: &Row) -> String {
    path(&labels(row, false), usize::MAX)
}

pub fn views(json: bool) -> Result<()> {
    let views = crate::tui::query::views(client::current()?.fields);
    if json {
        let list: Vec<_> = views
            .iter()
            .map(|(name, by)| json!({ "name": name, "group_by": by }))
            .collect();
        println!("{}", json!(list));
        return Ok(());
    }
    let width = views.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for (name, by) in views {
        match by {
            Some(by) => println!("{name:<width$}  groups by {by}"),
            None => println!("{name:<width$}  one list"),
        }
    }
    Ok(())
}
