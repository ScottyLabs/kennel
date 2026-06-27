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
    /// Maps a branch to its deployment environment
    pub fn from_branch(branch: &str) -> Option<Self> {
        match branch {
            "main" => Some(Self::Prod),
            "staging" => Some(Self::Staging),
            "dev" => Some(Self::Dev),
            s if s.starts_with("pr-") => Some(Self::Preview),
            _ => None,
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
