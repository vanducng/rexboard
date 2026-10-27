use super::Harness;

pub struct Grok;

impl Harness for Grok {
    fn id(&self) -> &'static str {
        "grok-build"
    }

    /// Its binary is named for its version and platform, as in `grok-1.0.70-darwin-arm64`.
    fn process_prefixes(&self) -> &'static [&'static str] {
        &["grok-"]
    }
}
