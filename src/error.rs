use std::process::ExitCode;

use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Usage { message: String, steps: Vec<String> },
    #[error("a command is required")]
    Help,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    RexNotFound(String),
    #[error("{0}")]
    Rex(String),
    #[error("{0}")]
    HostsUnreachable(String),
    #[error("{0}")]
    ContractMismatch(String),
    #[error("{message}")]
    Serve { message: String, steps: Vec<String> },
    #[error("{0}")]
    ServeRunning(String),
    #[error("{0}")]
    NotFound(String),
    #[error("nobody needs you")]
    NobodyNeedsYou,
    /// No app, or not exactly one, could take a jump on this host.
    #[error("{message}")]
    AmbiguousClient { message: String, steps: Vec<String> },
    /// Rex failed in a way rexboard can explain; `detail` keeps Rex's own words for --json.
    #[error("{message}")]
    RexExplained {
        message: String,
        steps: Vec<String>,
        detail: String,
    },
    #[error("{0}")]
    Config(String),
    /// The upgrade could not run, or did not take.
    #[error("{message}")]
    Upgrade { message: String, steps: Vec<String> },
    /// An extension could not be found, made or run, or its answer broke the contract.
    #[error("{message}")]
    Extension { message: String, steps: Vec<String> },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// serve did not start, answer or stop.
    pub fn serve(message: impl Into<String>) -> Self {
        Self::serve_with(message, &["rexboard serve --stop", "rexboard ls --direct"])
    }

    pub fn serve_with(message: impl Into<String>, steps: &[&str]) -> Self {
        Self::Serve {
            message: message.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn upgrade(message: impl Into<String>, steps: &[&str]) -> Self {
        Self::Upgrade {
            message: message.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn extension(message: impl Into<String>, steps: &[&str]) -> Self {
        Self::Extension {
            message: message.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn usage_with(message: impl Into<String>, steps: &[&str]) -> Self {
        Self::Usage {
            message: message.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage { .. } | Self::Help => "USAGE",
            Self::Cancelled => "CANCELLED",
            Self::RexNotFound(_) => "REX_NOT_FOUND",
            Self::Rex(_) | Self::RexExplained { .. } => "REX",
            Self::HostsUnreachable(_) => "HOSTS_UNREACHABLE",
            Self::ContractMismatch(_) => "CONTRACT_MISMATCH",
            Self::Serve { .. } => "SERVE",
            Self::ServeRunning(_) => "SERVE_RUNNING",
            Self::NotFound(_) => "NOT_FOUND",
            Self::NobodyNeedsYou => "NOBODY_NEEDS_YOU",
            Self::AmbiguousClient { .. } => "AMBIGUOUS_CLIENT",
            Self::Config(_) => "CONFIG",
            Self::Upgrade { .. } => "UPGRADE",
            Self::Extension { .. } => "EXTENSION",
            Self::Io(_) => "IO",
        }
    }

    pub fn next_steps(&self) -> Vec<String> {
        match self {
            Self::Usage { steps, .. } if !steps.is_empty() => steps.clone(),
            Self::RexNotFound(_) => vec![
                "set REXBOARD_REX to the path of the rex CLI".to_owned(),
                "install Rex".to_owned(),
            ],
            Self::Rex(_) => vec!["rex hosts".to_owned(), "retry the command once".to_owned()],
            Self::HostsUnreachable(_) => vec!["rex hosts".to_owned(), "rexboard ls".to_owned()],
            Self::Serve { steps, .. } => steps.clone(),
            Self::ServeRunning(_) => vec!["rexboard serve --stop".to_owned()],
            Self::NotFound(_) => vec!["rexboard ls --json".to_owned()],
            Self::NobodyNeedsYou => vec!["rexboard ls".to_owned()],
            Self::AmbiguousClient { steps, .. }
            | Self::RexExplained { steps, .. }
            | Self::Upgrade { steps, .. }
            | Self::Extension { steps, .. }
                if !steps.is_empty() =>
            {
                steps.clone()
            }
            Self::Config(_) => vec![CONFIG_DOC.to_owned()],
            Self::ContractMismatch(_) => vec![
                "rexboard --version".to_owned(),
                "rexboard upgrade".to_owned(),
            ],
            _ => vec!["rexboard --help".to_owned()],
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage { .. } | Self::Help | Self::HostsUnreachable(_) => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    }

    pub fn quiet(&self) -> bool {
        matches!(self, Self::Help | Self::Cancelled)
    }
}

const CONFIG_DOC: &str = "https://github.com/vanducng/rexboard/blob/main/docs/configuration.md";

pub fn report(json: bool, err: &Error) {
    if json {
        let mut body = json!({
            "ok": false,
            "error": {
                "code": err.code(),
                "message": err.to_string(),
                "next_steps": err.next_steps(),
            }
        });
        if let Error::RexExplained { detail, .. } = err {
            body["error"]["detail"] = json!(detail);
        }
        eprintln!("{body}");
        return;
    }
    if let Error::NobodyNeedsYou = err {
        eprintln!("{err}");
        return;
    }
    eprintln!("error: {err}");
    if let Some(step) = err.next_steps().first() {
        eprintln!("next: {step}");
    }
}

pub type Result<T> = std::result::Result<T, Error>;
