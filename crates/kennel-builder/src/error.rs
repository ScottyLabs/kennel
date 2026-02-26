use thiserror::Error;

#[derive(Error, Debug)]
pub enum BuilderError {
    #[error("git operation failed: {0}")]
    Git(String),

    #[error("nix build failed: {0}")]
    NixBuild(String),

    #[error("build cancelled")]
    Cancelled,

    #[error("invalid store path: {0}")]
    InvalidStorePath(String),

    #[error(transparent)]
    Store(#[from] kennel_store::StoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, BuilderError>;
