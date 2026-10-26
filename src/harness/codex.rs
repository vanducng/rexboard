use super::Harness;

pub struct Codex;

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn processes(&self) -> &'static [&'static str] {
        &["codex"]
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
