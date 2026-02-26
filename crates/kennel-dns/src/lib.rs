mod cloudflare;
mod error;
mod manager;
mod provider;

pub use cloudflare::CloudflareProvider;
pub use error::{Error, Result};
pub use manager::DnsManager;
pub use provider::{DnsProvider, DnsRecord, RecordType};
