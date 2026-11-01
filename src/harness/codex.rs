use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::Harness;
use super::usage::{Usage, effort, percent};
use crate::tui::icons::{Icon, icon};

/// Only the end of a rollout is read: they grow to hundreds of megabytes.
pub(super) const TAIL: u64 = 64 * 1024;
/// The effort is in the last `turn_context`, written when a turn starts, so after a long turn it
/// is further back than the tail: read back this far, in TAIL steps, and say nothing past it.
const EFFORT_BACK: u64 = 2 * 1024 * 1024;
/// The tokens Codex counts as its own fixed prompt and leaves out of the percent its footer shows.
const BASELINE: u64 = 12_000;

pub struct Codex;

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn mark(&self) -> Option<Icon> {
        Some(icon("cod-openai", "\u{ec81}"))
    }

    fn processes(&self) -> &'static [&'static str] {
        &["codex"]
    }

    fn usage_hook(&self) -> Option<&'static str> {
        Some("Stop")
    }

    /// Its Stop hook's JSON, which names the model and the rollout; the numbers come from the
    /// rollout's last `token_count` event, as the footer shows them.
    fn usage(&self, payload: &serde_json::Value) -> Option<Usage> {
        let (event, effort) = payload["transcript_path"]
            .as_str()
            .map_or((None, None), |path| rollout(Path::new(path)));
        let info = event.as_ref().map(|e| &e["payload"]["info"]);
        let last = info.map(|info| &info["last_token_usage"]);
        let window = info.and_then(|info| info["model_context_window"].as_u64());
        let ctx_pct = last
            .and_then(|last| last["total_tokens"].as_u64())
            .zip(window.filter(|w| *w > BASELINE))
            .map(|(total, window)| {
                let used = total.saturating_sub(BASELINE) as f64 / (window - BASELINE) as f64;
                (used * 100.0).round().clamp(0.0, 100.0) as u8
            });
        let primary = event
            .as_ref()
            .map(|e| &e["payload"]["rate_limits"]["primary"]);
        let usage = Usage {
            model: payload["model"].as_str().map(str::to_owned),
            model_display: None,
            effort,
            ctx_pct,
            ctx_window: window,
            tok: last.and_then(|last| last["input_tokens"].as_u64()),
            cost_usd: None,
            quota_5h: primary
                .filter(|p| p["window_minutes"].as_u64() == Some(300))
                .and_then(|p| percent(&p["used_percent"])),
        };
        (!usage.is_empty()).then_some(usage)
    }

    /// Codex in tmux reports nothing and titles the pane `<dir> | <model>`, as in `api | gpt-4x`.
    /// Strict, so a shell command with a pipe (`env | sort`) never matches: the pane must be tmux
    /// and the model must have a digit.
    fn title_hint(&self, title: &str, process: Option<&str>) -> bool {
        let Some((dir, model)) = title.split_once(" | ") else {
            return false;
        };
        process == Some("tmux")
            && title.matches('|').count() == 1
            && !dir.is_empty()
            && !dir.contains(char::is_whitespace)
            && !model.is_empty()
            && model.chars().any(|c| c.is_ascii_alphabetic())
            && model.chars().any(|c| c.is_ascii_digit())
            && model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    }
}

/// The last `token_count` event in the rollout's last TAIL bytes, and the effort of its last
/// `turn_context` within EFFORT_BACK bytes of the end, read from the end in TAIL steps.
fn rollout(path: &Path) -> (Option<serde_json::Value>, Option<String>) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (None, None);
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let (mut count, mut found, mut context) = (None, None, false);
    let (mut end, mut carry) = (len, Vec::new());
    while end > 0 && len - end < EFFORT_BACK && !context {
        let start = end.saturating_sub(TAIL);
        let mut chunk = vec![0; (end - start) as usize];
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut chunk).is_err() {
            break;
        }
        chunk.extend(&carry);
        // The last token_count is the one in the last TAIL bytes, a line that starts at the
        // chunk's first byte included; a cut one does not parse and is skipped.
        if end == len {
            count = String::from_utf8_lossy(&chunk)
                .lines()
                .rev()
                .filter(|line| line.contains("\"token_count\""))
                .find_map(|line| {
                    let event: serde_json::Value = serde_json::from_str(line).ok()?;
                    (event["payload"]["type"] == "token_count").then_some(event)
                });
        }
        // A line the chunk's start cuts is finished by the next, earlier chunk.
        let first_line = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |at| at + 1);
        let whole = chunk.split_off(if start > 0 { first_line } else { 0 });
        carry = chunk;
        carry.pop_if(|b| *b == b'\n');
        for line in String::from_utf8_lossy(&whole).lines().rev() {
            if !context && line.contains("\"turn_context\"") {
                let event = serde_json::from_str::<serde_json::Value>(line).ok();
                // The latest turn_context speaks for the session, whether it has an effort or
                // not: an older one's is never what it is running now.
                if let Some(event) = event.filter(|e| e["type"] == "turn_context") {
                    context = true;
                    found = event["payload"]["effort"].as_str().and_then(effort);
                }
            }
        }
        end = start;
    }
    (count, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_title_is_strict() {
        let title = |title: &str, process: Option<&str>| Codex.title_hint(title, process);
        let tmux = Some("tmux");
        assert!(title("api | gpt-x1", tmux));
        assert!(title("work | Model-2.5-Pro", tmux));
        assert!(!title("api | gpt-x1", Some("zsh")), "only a tmux pane");
        assert!(!title("api | gpt-x1", None), "only a tmux pane");
        assert!(!title("env | sort", tmux), "a model has a digit");
        assert!(!title("dmesg | less", tmux), "a model has a digit");
        assert!(!title("history | tail", tmux), "a model has a digit");
        assert!(!title("tail -f log | grep err", tmux), "a shell pipe");
        assert!(!title("a | b1 | c", tmux), "two separators");
        assert!(!title("api | gpt x1", tmux), "a space in the model");
        assert!(!title("api|gpt-x1", tmux), "no spaces around the bar");
        assert!(!title(" | gpt-x1", tmux), "no dir");
        assert!(!title("api | 6.1", tmux), "a model has letters");
    }
}
