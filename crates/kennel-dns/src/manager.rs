use crate::provider::{DnsProvider, RecordType};
use crate::{Error, Result};
use kennel_store::Store;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct DnsManager {
    providers: HashMap<String, Arc<dyn DnsProvider>>,
    store: Arc<Store>,
    server_ipv4: Ipv4Addr,
    server_ipv6: Ipv6Addr,
}

impl DnsManager {
    pub fn new(
        providers: HashMap<String, Arc<dyn DnsProvider>>,
        store: Arc<Store>,
        server_ipv4: Ipv4Addr,
        server_ipv6: Ipv6Addr,
    ) -> Self {
        Self {
            providers,
            store,
            server_ipv4,
            server_ipv6,
        }
    }

    fn get_provider_for_domain(&self, domain: &str) -> Result<&Arc<dyn DnsProvider>> {
        for (zone, provider) in &self.providers {
            if domain.ends_with(zone) {
                return Ok(provider);
            }
        }
        Err(Error::NoProviderForDomain(domain.to_string()))
    }

    pub async fn create_record_for_deployment(
        &self,
        deployment_id: i32,
        domain: &str,
    ) -> Result<()> {
        let provider = self.get_provider_for_domain(domain)?;

        info!(
            "Creating DNS records for deployment {} (domain: {})",
            deployment_id, domain
        );

        // Always create both A and AAAA records
        let a_record = provider
            .create_record(domain, RecordType::A, &self.server_ipv4.to_string())
            .await?;

        self.store
            .dns_records()
            .create(
                domain,
                deployment_id,
                &a_record.provider_record_id,
                "A",
                &self.server_ipv4.to_string(),
            )
            .await?;

        let aaaa_record = provider
            .create_record(domain, RecordType::AAAA, &self.server_ipv6.to_string())
            .await?;

        self.store
            .dns_records()
            .create(
                domain,
                deployment_id,
                &aaaa_record.provider_record_id,
                "AAAA",
                &self.server_ipv6.to_string(),
            )
            .await?;

        info!("DNS records created successfully for {}", domain);

        Ok(())
    }

    pub async fn delete_record_for_deployment(&self, deployment_id: i32) -> Result<()> {
        let records = self
            .store
            .dns_records()
            .find_by_deployment(deployment_id)
            .await?;

        for record in records {
            info!(
                "Deleting DNS record: {} ({})",
                record.domain, record.record_type
            );

            let provider = self.get_provider_for_domain(&record.domain)?;

            if let Err(e) = provider.delete_record(&record.provider_record_id).await {
                error!(
                    "Failed to delete DNS record {} from provider: {}",
                    record.provider_record_id, e
                );
            }

            self.store.dns_records().delete(record.id).await?;
        }

        Ok(())
    }

    pub async fn create_wildcard_for_project(
        &self,
        project_name: &str,
        base_domain: &str,
    ) -> Result<()> {
        let wildcard_domain = format!("*.{}.{}", project_name, base_domain);
        let provider = self.get_provider_for_domain(&wildcard_domain)?;

        info!("Creating wildcard DNS for project: {}", wildcard_domain);

        // Create both A and AAAA wildcard records
        let a_record = provider
            .create_record(
                &wildcard_domain,
                RecordType::A,
                &self.server_ipv4.to_string(),
            )
            .await?;

        self.store
            .dns_records()
            .create(
                &wildcard_domain,
                None,
                &a_record.provider_record_id,
                "A",
                &self.server_ipv4.to_string(),
            )
            .await?;

        let aaaa_record = provider
            .create_record(
                &wildcard_domain,
                RecordType::AAAA,
                &self.server_ipv6.to_string(),
            )
            .await?;

        self.store
            .dns_records()
            .create(
                &wildcard_domain,
                None,
                &aaaa_record.provider_record_id,
                "AAAA",
                &self.server_ipv6.to_string(),
            )
            .await?;

        info!("Wildcard DNS created successfully for {}", wildcard_domain);

        Ok(())
    }

    pub async fn delete_wildcard_for_project(
        &self,
        project_name: &str,
        base_domain: &str,
    ) -> Result<()> {
        let wildcard_domain = format!("*.{}.{}", project_name, base_domain);

        let records = self
            .store
            .dns_records()
            .find_by_domain(&wildcard_domain)
            .await?;

        for record in records {
            info!(
                "Deleting wildcard DNS record: {} ({})",
                record.domain, record.record_type
            );

            let provider = self.get_provider_for_domain(&record.domain)?;

            if let Err(e) = provider.delete_record(&record.provider_record_id).await {
                error!(
                    "Failed to delete wildcard DNS record {} from provider: {}",
                    record.provider_record_id, e
                );
            }

            self.store.dns_records().delete(record.id).await?;
        }

        Ok(())
    }

    pub async fn reconcile(&self) -> Result<ReconciliationSummary> {
        let mut summary = ReconciliationSummary::default();

        info!("Starting DNS reconciliation");

        // Find deployments with pending DNS
        let deployments = self
            .store
            .deployments()
            .find_by_dns_status("pending")
            .await?;

        for deployment in deployments {
            match self
                .create_record_for_deployment(deployment.id, &deployment.domain)
                .await
            {
                Ok(_) => {
                    self.store
                        .deployments()
                        .update_dns_status(deployment.id, "active")
                        .await?;
                    summary.dns_created += 1;
                }
                Err(e) => {
                    error!(
                        "Failed to create DNS for deployment {}: {}",
                        deployment.id, e
                    );
                    summary.dns_failed += 1;
                }
            }
        }

        // Find orphaned DNS records
        for (zone, provider) in &self.providers {
            info!("Checking for orphaned DNS records in zone {}", zone);

            let provider_records = match provider.list_records().await {
                Ok(records) => records,
                Err(e) => {
                    error!("Failed to list DNS records for zone {}: {}", zone, e);
                    continue;
                }
            };

            let our_records = self.store.dns_records().find_all().await?;

            for provider_record in provider_records {
                if !our_records
                    .iter()
                    .any(|r| r.provider_record_id == provider_record.provider_record_id)
                {
                    warn!(
                        "Found orphaned DNS record: {}, deleting",
                        provider_record.name
                    );

                    if let Err(e) = provider
                        .delete_record(&provider_record.provider_record_id)
                        .await
                    {
                        error!(
                            "Failed to delete orphaned DNS record {}: {}",
                            provider_record.name, e
                        );
                    } else {
                        summary.dns_orphaned += 1;
                    }
                }
            }
        }

        info!(
            "DNS reconciliation complete: {} created, {} failed, {} orphaned",
            summary.dns_created, summary.dns_failed, summary.dns_orphaned
        );

        Ok(summary)
    }
}

#[derive(Debug, Default)]
pub struct ReconciliationSummary {
    pub dns_created: usize,
    pub dns_failed: usize,
    pub dns_orphaned: usize,
}
