use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Prod,
    Staging,
    Dev,
    Preview,
}

impl Environment {
    pub fn from_branch(branch: &str) -> Self {
        match branch {
            "main" => Self::Prod,
            "staging" => Self::Staging,
            "dev" => Self::Dev,
            s if s.starts_with("pr-") => Self::Preview,
            _ => Self::Dev,
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prod => write!(f, "prod"),
            Self::Staging => write!(f, "staging"),
            Self::Dev => write!(f, "dev"),
            Self::Preview => write!(f, "preview"),
        }
    }
}
