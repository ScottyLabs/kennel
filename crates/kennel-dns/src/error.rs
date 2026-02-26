use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Cloudflare API error: {0}")]
    CloudflareApi(#[from] cloudflare::framework::Error),

    #[error("Cloudflare API failure: {0}")]
    CloudflareApiFailure(#[from] cloudflare::framework::response::ApiFailure),

    #[error("Database error: {0}")]
    Database(#[from] kennel_store::StoreError),

    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(#[from] std::net::AddrParseError),

    #[error("No DNS provider configured for domain: {0}")]
    NoProviderForDomain(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
