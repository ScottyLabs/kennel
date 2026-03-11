use thiserror::Error;

#[derive(Error, Debug)]
pub enum DeployerError {
    #[error("deployment not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Supervisor(#[from] kennel_supervisor::SupervisorError),

    #[error(transparent)]
    Store(#[from] kennel_store::StoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DeployerError>;
