use std::process::ExitCode;

use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Usage { message: String, steps: Vec<String> },
    #[error("a command is required")]
    Help,
    #[error("{0}")]
    RexNotFound(String),
    #[error("{0}")]
    Rex(String),
    #[error("{0}")]
    HostsUnreachable(String),
    #[error("{0}")]
    ContractMismatch(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn usage_with(message: impl Into<String>, steps: &[&str]) -> Self {
        Self::Usage {
            message: message.into(),
            steps: steps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage { .. } | Self::Help => "USAGE",
            Self::RexNotFound(_) => "REX_NOT_FOUND",
            Self::Rex(_) => "REX",
            Self::HostsUnreachable(_) => "HOSTS_UNREACHABLE",
            Self::ContractMismatch(_) => "CONTRACT_MISMATCH",
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
            Self::ContractMismatch(_) => vec![
                "rexboard --version".to_owned(),
                "install the latest rexboard release".to_owned(),
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
        matches!(self, Self::Help)
    }
}

pub fn report(json: bool, err: &Error) {
    if json {
        let body = json!({
            "ok": false,
            "error": {
                "code": err.code(),
                "message": err.to_string(),
                "next_steps": err.next_steps(),
            }
        });
        eprintln!("{body}");
        return;
    }
    eprintln!("error: {err}");
    if let Some(step) = err.next_steps().first() {
        eprintln!("next: {step}");
    }
}

pub type Result<T> = std::result::Result<T, Error>;
