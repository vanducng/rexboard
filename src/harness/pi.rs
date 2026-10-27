use super::Harness;
use super::usage::{Usage, percent};

pub struct Pi;

impl Harness for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn processes(&self) -> &'static [&'static str] {
        &["pi"]
    }

    /// What its extension sends at a turn's end: `{"model", "context": ctx.getContextUsage(),
    /// "usage", "cost"}`, where the context is `{tokens, contextWindow, percent}` and `usage` is
    /// the last assistant message's. TOK is that message's prompt side, as for every harness:
    /// the context's `tokens` counts its reply too.
    fn usage(&self, payload: &serde_json::Value) -> Option<Usage> {
        let ctx = &payload["context"];
        let sent = &payload["usage"];
        let prompt = ["input", "cacheRead", "cacheWrite"]
            .iter()
            .map(|part| sent[part].as_u64())
            .sum::<Option<u64>>();
        let usage = Usage {
            model: payload["model"].as_str().map(str::to_owned),
            ctx_pct: percent(&ctx["percent"]),
            ctx_window: ctx["contextWindow"].as_u64(),
            tok: prompt.filter(|tok| *tok > 0),
            cost_usd: payload["cost"].as_f64().filter(|cost| *cost > 0.0),
            quota_5h: None,
        };
        (!usage.is_empty()).then_some(usage)
    }
}
