use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouterError {
    #[error("route not found: {0}")]
    NotFound(String),

    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("proxy error: {0}")]
    Proxy(String),

    #[error(transparent)]
    Store(#[from] kennel_store::StoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, RouterError>;
