use crate::Result;
use async_trait::async_trait;

#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn create_record(
        &self,
        name: &str,
        record_type: RecordType,
        content: &str,
    ) -> Result<DnsRecord>;

    async fn delete_record(&self, provider_record_id: &str) -> Result<()>;

    async fn list_records(&self) -> Result<Vec<DnsRecord>>;
}

#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub provider_record_id: String,
    pub name: String,
    pub record_type: RecordType,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    A,
    AAAA,
}

impl RecordType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecordType::A => "A",
            RecordType::AAAA => "AAAA",
        }
    }
}

impl std::fmt::Display for RecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for RecordType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "A" => Ok(RecordType::A),
            "AAAA" => Ok(RecordType::AAAA),
            _ => Err(anyhow::anyhow!("Invalid record type: {}", s)),
        }
    }
}
