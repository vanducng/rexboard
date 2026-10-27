use super::Harness;

pub struct Claude;

impl Harness for Claude {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn name(&self) -> &'static str {
        "claude"
    }

    fn processes(&self) -> &'static [&'static str] {
        &["claude"]
    }

    /// Claude Code's binary is named for its version, as in `2.1.310`.
    fn is_process(&self, name: &str) -> bool {
        self.processes().contains(&name) || is_version(name)
    }

    /// Its status line JSON, which carries the model, the context window and the cost.
    fn usage(&self, payload: &serde_json::Value) -> Option<super::usage::Usage> {
        super::usage::status_line(payload)
    }

    /// Any state glyph at the start of the title, which Claude Code and the seat status line set.
    fn title_hint(&self, title: &str, _process: Option<&str>) -> bool {
        crate::pane::title_state(title).is_some()
    }
}

fn is_version(name: &str) -> bool {
    name.split('.').count() == 3
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_string_process_is_claude() {
        assert_eq!(
            crate::harness::name_of(None, Some("2.1.310"), "").as_deref(),
            Some("claude-code")
        );
        assert_eq!(crate::harness::name_of(None, Some("2.1"), ""), None);
        assert_eq!(crate::harness::name_of(None, Some("zsh"), ""), None);
    }
}
