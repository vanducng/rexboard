//! The agent harnesses rexboard knows: how a pane is recognised as one, and what its screen looks
//! like. Each built-in is a unit type in its own file; every method but `id` defaults to the
//! shared behaviour, so an adapter states only how it differs, and only with a captured fixture.

mod claude;
mod codex;
pub mod dialog;
mod grok;
mod pi;

pub trait Harness: Sync {
    /// The wire id: the Rex record `app` its hooks write. Never renamed.
    fn id(&self) -> &'static str;

    /// What the board and `ls` call it: short, for the HARNESS column.
    fn name(&self) -> &'static str {
        self.id()
    }

    /// Process names that mean this harness is in front.
    fn processes(&self) -> &'static [&'static str] {
        &[]
    }

    /// Starts of process names that carry a version.
    fn process_prefixes(&self) -> &'static [&'static str] {
        &[]
    }

    fn is_process(&self, name: &str) -> bool {
        self.processes().contains(&name)
            || self.process_prefixes().iter().any(|p| name.starts_with(p))
    }

    /// Weaker evidence from the pane's title, checked after the record and the process.
    fn title_hint(&self, _title: &str, _process: Option<&str>) -> bool {
        false
    }
}

/// In title order: Claude's glyph is checked before Codex's title.
pub static BUILTIN: &[&dyn Harness] = &[&claude::Claude, &codex::Codex, &pi::Pi, &grok::Grok];

/// The display name for a wire id; an id rexboard does not know shows as it is.
pub fn display(id: &str) -> &str {
    BUILTIN
        .iter()
        .find(|h| h.id() == id)
        .map_or(id, |h| h.name())
}

/// The process names an agent runs as, besides Claude Code's binary named for its version.
pub fn processes() -> Vec<&'static str> {
    BUILTIN
        .iter()
        .flat_map(|h| h.processes())
        .copied()
        .collect()
}

pub fn process_prefixes() -> Vec<&'static str> {
    BUILTIN
        .iter()
        .flat_map(|h| h.process_prefixes())
        .copied()
        .collect()
}

/// Which harness a pane runs: its record's app, else the process in front, else its title. An
/// app rexboard does not know is kept as it is.
pub fn name_of(app: Option<String>, process: Option<&str>, title: &str) -> Option<String> {
    let find = |found: &dyn Fn(&dyn Harness) -> bool| {
        BUILTIN
            .iter()
            .find(|h| found(**h))
            .map(|h| h.id().to_owned())
    };
    app.or_else(|| process.and_then(|p| find(&|h| h.is_process(p))))
        .or_else(|| find(&|h| h.title_hint(title, process)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_harness_shows_its_short_name_and_an_unknown_one_its_id() {
        assert_eq!(display("claude-code"), "claude");
        assert_eq!(display("grok-build"), "grok");
        assert_eq!(display("codex"), "codex");
        assert_eq!(display("opencode"), "opencode");
    }
}
