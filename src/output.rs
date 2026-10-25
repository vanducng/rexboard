use chrono::{DateTime, Utc};

use crate::pane::{Pane, bare_title};

pub fn table(panes: &[Pane], now: DateTime<Utc>) -> String {
    let rows: Vec<[String; 6]> = panes
        .iter()
        .map(|pane| {
            [
                pane.state.clone(),
                pane.updated_at
                    .as_deref()
                    .map_or("-".into(), |at| age(at, now)),
                pane.harness.clone().unwrap_or_else(|| "-".into()),
                pane.host.clone(),
                format!("{} › {} › {}", pane.session, pane.tab, pane.pane),
                pane.msg
                    .clone()
                    .filter(|msg| !msg.is_empty())
                    .unwrap_or_else(|| bare_title(&pane.title).to_owned()),
            ]
        })
        .collect();
    let header = ["STATE", "AGE", "HARNESS", "HOST", "WHERE", "WHAT"].map(String::from);
    let mut widths = [0; 6];
    for row in std::iter::once(&header).chain(&rows) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if i + 1 == row.len() {
                line.push_str(cell);
            } else {
                line.push_str(cell);
                line.push_str(&" ".repeat(widths[i] - cell.chars().count() + 2));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn age(at: &str, now: DateTime<Utc>) -> String {
    let Ok(at) = DateTime::parse_from_rfc3339(at) else {
        return "-".into();
    };
    let secs = (now - at.with_timezone(&Utc)).num_seconds().max(0);
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_picks_the_largest_unit() {
        let now = DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(age("2026-01-01T23:59:30Z", now), "30s");
        assert_eq!(age("2026-01-01T23:48:00+00:00", now), "12m");
        assert_eq!(age("2026-01-02T04:00:00+07:00", now), "3h");
        assert_eq!(age("2025-12-30T00:00:00Z", now), "3d");
        assert_eq!(age("not a time", now), "-");
    }
}
