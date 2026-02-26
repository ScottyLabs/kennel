use crate::Result;
use crate::provider::{DnsProvider, DnsRecord, RecordType};
use async_trait::async_trait;
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DeleteDnsRecord, DnsContent, ListDnsRecords,
    ListDnsRecordsParams,
};
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::ClientConfig;
use cloudflare::framework::client::async_api::Client;

pub struct CloudflareProvider {
    client: Client,
    zone_identifier: String,
}

impl CloudflareProvider {
    pub fn new(api_token: String, zone_id: String) -> Result<Self> {
        let credentials = Credentials::UserAuthToken { token: api_token };
        let client = Client::new(
            credentials,
            ClientConfig::default(),
            Environment::Production,
        )?;

        Ok(Self {
            client,
            zone_identifier: zone_id,
        })
    }
}

#[async_trait]
impl DnsProvider for CloudflareProvider {
    async fn create_record(
        &self,
        name: &str,
        record_type: RecordType,
        content: &str,
    ) -> Result<DnsRecord> {
        let dns_content = match record_type {
            RecordType::A => DnsContent::A {
                content: content.parse()?,
            },
            RecordType::AAAA => DnsContent::AAAA {
                content: content.parse()?,
            },
        };

        let params = CreateDnsRecordParams {
            name,
            content: dns_content,
            ttl: Some(300),
            proxied: Some(true),
            priority: None,
        };

        let response = self
            .client
            .request(&CreateDnsRecord {
                zone_identifier: &self.zone_identifier,
                params,
            })
            .await?;

        Ok(DnsRecord {
            provider_record_id: response.result.id,
            name: response.result.name,
            record_type,
            content: content.to_string(),
        })
    }

    async fn delete_record(&self, provider_record_id: &str) -> Result<()> {
        self.client
            .request(&DeleteDnsRecord {
                zone_identifier: &self.zone_identifier,
                identifier: provider_record_id,
            })
            .await?;

        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<DnsRecord>> {
        let response = self
            .client
            .request(&ListDnsRecords {
                zone_identifier: &self.zone_identifier,
                params: ListDnsRecordsParams::default(),
            })
            .await?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|r| {
                let (record_type, content) = match r.content {
                    DnsContent::A { content } => (RecordType::A, content.to_string()),
                    DnsContent::AAAA { content } => (RecordType::AAAA, content.to_string()),
                    _ => return None,
                };

                Some(DnsRecord {
                    provider_record_id: r.id,
                    name: r.name,
                    record_type,
                    content,
                })
            })
            .collect())
    }
}
