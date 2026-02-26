use thiserror::Error;

#[derive(Error, Debug)]
pub enum DeployerError {
    #[error("systemd operation failed: {0}")]
    Systemd(String),

    #[error("health check failed: {0}")]
    HealthCheck(String),

    #[error("deployment not found: {0}")]
    NotFound(String),

    #[error("port allocation failed: {0}")]
    PortAllocation(String),

    #[error(transparent)]
    Store(#[from] kennel_store::StoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DeployerError>;
