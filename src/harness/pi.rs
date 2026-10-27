use super::Harness;

pub struct Pi;

impl Harness for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn processes(&self) -> &'static [&'static str] {
        &["pi"]
    }
}
