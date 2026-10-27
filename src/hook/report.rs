//! A hook event as a Rex status: which state it means, and the OSC 7501 sequence that tells Rex.
//! A byte-for-byte port of the `rex-agent-status` bridge, checked against its own output in
//! `tests/fixtures/hooks/corpus.jsonl`.

use serde_json::Value;

/// Rex's limit on `msg`, in decoded bytes.
const MSG_MAX: usize = 2048;
/// Rex's limit on the whole sequence; a report over either is dropped whole.
const SEQ_MAX: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub state: &'static str,
    pub kind: Option<&'static str>,
    pub msg: Option<String>,
}

fn status_of(state: &'static str, kind: Option<&'static str>, msg: Option<String>) -> Status {
    Status { state, kind, msg }
}

/// The status a hook event means for `app`, or None to leave the status alone.
pub fn status(event: &Value, app: &str) -> Option<Status> {
    let name = event["hook_event_name"].as_str().unwrap_or_default();
    let tool = event["tool_name"].as_str();
    let args = &event["tool_input"];
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    Some(match (name, tool) {
        ("SessionStart", _) => status_of("idle", None, None),
        ("UserPromptSubmit", _) => status_of("working", None, Some("Working".into())),
        ("PreToolUse", Some("AskUserQuestion")) => {
            status_of("blocked", Some("question"), Some(question(args)))
        }
        ("PreToolUse", _) => status_of(
            "working",
            None,
            text(&args["description"]).or(tool.filter(|t| !t.is_empty()).map(str::to_owned)),
        ),
        ("PostToolUse", Some("AskUserQuestion")) => {
            status_of("working", None, Some("Working".into()))
        }
        ("PermissionRequest", _) => {
            let asked = (app == "codex")
                .then(|| text(&args["description"]))
                .flatten();
            let tool = tool.filter(|t| !t.is_empty()).unwrap_or("a tool");
            let msg = asked.unwrap_or_else(|| format!("Allow {tool}?"));
            status_of("blocked", Some("permission"), Some(msg))
        }
        ("Notification", _) if event["notification_type"] == "permission_prompt" => status_of(
            "blocked",
            Some("permission"),
            event["message"].as_str().map(str::to_owned),
        ),
        ("Stop", _) => {
            let said = one_line(event["last_assistant_message"].as_str().unwrap_or_default());
            status_of("done", None, Some(non_empty(said).unwrap_or("Done".into())))
        }
        ("StopFailure", _) => {
            let said = one_line(event["last_assistant_message"].as_str().unwrap_or_default());
            let why: Vec<&str> = [&event["error"], &event["error_details"]]
                .into_iter()
                .filter_map(|part| part.as_str().filter(|s| !s.is_empty()))
                .collect();
            let msg = non_empty(said)
                .or_else(|| non_empty(why.join(" ")))
                .unwrap_or("error".into());
            status_of("error", None, Some(msg))
        }
        ("SessionEnd", _) => status_of("clear", None, None),
        _ => return None,
    })
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// In a plain Rex pane, Claude's own `done` carries no text, so the final message also goes to a
/// `turn` sub-record, cleared when the next prompt starts or the session ends.
pub fn turn_record(app: &str, name: &str, status: &Status) -> Option<String> {
    match name {
        "Stop" => Some(fields(
            app,
            "done",
            None,
            status.msg.as_deref(),
            Some("turn"),
        )),
        "UserPromptSubmit" | "SessionEnd" => Some(fields(app, "clear", None, None, Some("turn"))),
        _ => None,
    }
}

/// An AskUserQuestion as one msg: the first question, then its options, numbered.
fn question(args: &Value) -> String {
    let Some(questions) = args["questions"].as_array().filter(|q| !q.is_empty()) else {
        return "Question".into();
    };
    let first = &questions[0];
    let said = |key: &str| first[key].as_str().filter(|s| !s.is_empty());
    let mut lines = vec![
        said("question")
            .or(said("header"))
            .unwrap_or("Question")
            .to_owned(),
    ];
    for (n, option) in first["options"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let label = option["label"].as_str().unwrap_or("None");
        lines.push(
            match option["description"].as_str().filter(|s| !s.is_empty()) {
                Some(about) => format!("{}. {label} - {about}", n + 1),
                None => format!("{}. {label}", n + 1),
            },
        );
    }
    if questions.len() > 1 {
        lines.push(format!("(+{} more questions)", questions.len() - 1));
    }
    one_line(&lines.join("\n"))
}

/// Rex allows no control characters in msg, so lines join with ` ⏎ `. A run of blank lines leaves
/// one empty line, so paragraphs survive, and none at the start or end. Leading spaces stay, for
/// code, with a leading tab as four; trailing ones go. Any other control character is a space.
pub fn one_line(text: &str) -> String {
    let control = |c: char| matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}');
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let mut out = String::with_capacity(line.len());
        let mut run = false;
        for c in line.chars() {
            if c == '\t' && out.chars().all(|c| c == ' ') {
                out.push_str("    ");
            } else if control(c) {
                if !run {
                    out.push(' ');
                }
                run = true;
                continue;
            } else {
                out.push(c);
            }
            run = false;
        }
        let out = out.trim_end();
        if !out.is_empty() || lines.last().is_some_and(|last| !last.is_empty()) {
            lines.push(out.to_owned());
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join(" ⏎ ")
}

/// `text` in at most `limit` UTF-8 bytes, cut at a character and ending in … when cut.
fn capped(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit - '…'.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The record's `key=value` pairs, its msg cut until the wrapped sequence fits Rex's limit.
pub fn fields(
    app: &str,
    state: &str,
    kind: Option<&str>,
    msg: Option<&str>,
    record: Option<&str>,
) -> String {
    let mut pairs: Vec<String> = record.map(|id| format!("id={id}")).into_iter().collect();
    pairs.push(format!("app={app}"));
    pairs.push(format!("state={state}"));
    pairs.extend(kind.map(|kind| format!("kind={kind}")));
    let line = one_line(msg.unwrap_or_default());
    let mut limit = MSG_MAX;
    loop {
        let mut out = pairs.clone();
        if !line.is_empty() {
            out.push(format!("msg={}", base64(capped(&line, limit).as_bytes())));
        }
        let out = out.join(":");
        if wrap(&out).len() <= SEQ_MAX {
            return out;
        }
        if limit <= 256 {
            return pairs.join(":");
        }
        limit -= 256;
    }
}

pub fn osc(pairs: &str) -> String {
    format!("\x1b]7501;{pairs}\x07")
}

/// The status in tmux passthrough, which forwards it to the attached Rex tile.
pub fn wrap(pairs: &str) -> String {
    format!("\x1bPtmux;{}\x1b\\", osc(pairs).replace('\x1b', "\x1b\x1b"))
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recorded_event_gives_the_bridges_own_bytes() {
        let corpus = include_str!("../../tests/fixtures/hooks/corpus.jsonl");
        let mut checked = 0;
        for line in corpus.lines() {
            let row: Value = serde_json::from_str(line).unwrap();
            let app = row["app"].as_str().unwrap();
            let payload = &row["payload"];
            let got = status(payload, app);
            let seen = got.as_ref().map(|s| (s.state, s.kind, s.msg.as_deref()));
            let want = row["status"]
                .as_array()
                .map(|st| (st[0].as_str().unwrap(), st[1].as_str(), st[2].as_str()));
            assert_eq!(seen, want, "{app} {payload}");
            let Some(got) = got else {
                continue;
            };
            let pairs = fields(app, got.state, got.kind, got.msg.as_deref(), None);
            assert_eq!(pairs, row["fields"], "{app} {payload}");
            assert_eq!(wrap(&pairs), row["tmux"], "{app} {payload}");
            assert_eq!(osc(&pairs), row["plain"], "{app} {payload}");
            let name = payload["hook_event_name"].as_str().unwrap_or_default();
            let turn = turn_record(app, name, &got).map(|r| osc(&r));
            assert_eq!(turn.as_deref(), row["turn"].as_str(), "{app} {payload}");
            checked += 1;
        }
        assert!(checked >= 70, "only {checked} events with a status");
    }

    #[test]
    fn a_message_keeps_its_paragraphs_and_indentation_on_one_line() {
        for (text, want) in [
            ("a\n\n\n\nb", "a ⏎  ⏎ b"),
            ("\n\n  a  \n\n", "  a"),
            (
                "fn x() {\n    y();\n\tz();\n}",
                "fn x() { ⏎     y(); ⏎     z(); ⏎ }",
            ),
            ("a\tb\u{7}c\r\nd", "a b c ⏎ d"),
            ("one line", "one line"),
            ("\n \t \n", ""),
        ] {
            assert_eq!(one_line(text), want, "{text:?}");
            assert_eq!(one_line(want), want, "a second pass changes nothing");
        }
    }

    #[test]
    fn base64_pads_as_the_standard_alphabet_does() {
        for (text, want) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v")] {
            assert_eq!(base64(text.as_bytes()), want);
        }
    }
}
