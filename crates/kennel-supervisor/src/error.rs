use thiserror::Error;

#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("process {name} failed: {reason}")]
    ProcessFailed { name: String, reason: String },

    #[error("dependency cycle detected")]
    DependencyCycle,

    #[error("unknown process: {0}")]
    UnknownProcess(String),

    #[error("socket bind failed: {0}")]
    SocketBind(String),

    #[error("readiness probe timed out for {0}")]
    ProbeTimeout(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SupervisorError>;
