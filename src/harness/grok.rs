use super::Harness;

pub struct Grok;

impl Harness for Grok {
    fn id(&self) -> &'static str {
        "grok-build"
    }

    fn name(&self) -> &'static str {
        "grok"
    }

    /// Its status line JSON, in Claude Code's shape: the model and how full the context is.
    fn usage(&self, payload: &serde_json::Value) -> Option<super::usage::Usage> {
        super::usage::status_line(payload)
    }

    /// Its binary is named for its version and platform, as in `grok-1.0.70-darwin-arm64`.
    fn process_prefixes(&self) -> &'static [&'static str] {
        &["grok-"]
    }
}
