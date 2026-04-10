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
    server_ipv6: Option<Ipv6Addr>,
}

impl DnsManager {
    pub fn new(
        providers: HashMap<String, Arc<dyn DnsProvider>>,
        store: Arc<Store>,
        server_ipv4: Ipv4Addr,
        server_ipv6: Option<Ipv6Addr>,
    ) -> Self {
        Self {
            providers,
            store,
            server_ipv4,
            server_ipv6,
        }
    }

    fn get_provider_for_domain(&self, domain: &str) -> Result<&Arc<dyn DnsProvider>> {
        // Find the longest-matching zone for this domain, checking boundary.
        let mut best: Option<(&str, &Arc<dyn DnsProvider>)> = None;
        for (zone, provider) in &self.providers {
            let matches = domain == zone.as_str() || domain.ends_with(&format!(".{zone}"));
            if matches && best.as_ref().map_or(true, |(z, _)| zone.len() > z.len()) {
                best = Some((zone.as_str(), provider));
            }
        }
        best.map(|(_, p)| p)
            .ok_or_else(|| Error::NoProviderForDomain(domain.to_string()))
    }

    pub async fn create_record_for_deployment(
        &self,
        deployment_id: uuid::Uuid,
        domain: &str,
    ) -> Result<()> {
        // Check for domain conflicts: another deployment already owns this domain.
        let existing = self.store.dns_records().find_by_domain(domain).await?;
        let conflict = existing
            .iter()
            .any(|r| r.deployment_id.is_some_and(|id| id != deployment_id));
        if conflict {
            return Err(Error::Other(anyhow::anyhow!(
                "domain '{domain}' is already claimed by another deployment"
            )));
        }

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
                entity::sea_orm_active_enums::RecordType::A,
                &self.server_ipv4.to_string(),
            )
            .await?;

        if let Some(ipv6) = self.server_ipv6 {
            let aaaa_record = provider
                .create_record(domain, RecordType::AAAA, &ipv6.to_string())
                .await?;

            self.store
                .dns_records()
                .create(
                    domain,
                    deployment_id,
                    &aaaa_record.provider_record_id,
                    entity::sea_orm_active_enums::RecordType::Aaaa,
                    &ipv6.to_string(),
                )
                .await?;
        }

        info!("DNS records created successfully for {}", domain);

        Ok(())
    }

    pub async fn delete_record_for_deployment(&self, deployment_id: uuid::Uuid) -> Result<()> {
        let records = self
            .store
            .dns_records()
            .find_by_deployment(deployment_id)
            .await?;

        for record in records {
            info!(
                "Deleting DNS record: {} ({:?})",
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

        let existing = self
            .store
            .dns_records()
            .find_by_domain(&wildcard_domain)
            .await?;
        if !existing.is_empty() {
            return Ok(());
        }

        let provider = self.get_provider_for_domain(&wildcard_domain)?;

        info!("Creating wildcard DNS for project: {}", wildcard_domain);

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
                entity::sea_orm_active_enums::RecordType::A,
                &self.server_ipv4.to_string(),
            )
            .await?;

        if let Some(ipv6) = self.server_ipv6 {
            let aaaa_record = provider
                .create_record(&wildcard_domain, RecordType::AAAA, &ipv6.to_string())
                .await?;

            self.store
                .dns_records()
                .create(
                    &wildcard_domain,
                    None,
                    &aaaa_record.provider_record_id,
                    entity::sea_orm_active_enums::RecordType::Aaaa,
                    &ipv6.to_string(),
                )
                .await?;
        }

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
                "Deleting wildcard DNS record: {} ({:?})",
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
            .find_by_dns_status(entity::sea_orm_active_enums::DnsStatus::Pending)
            .await?;

        for deployment in deployments {
            match self
                .create_record_for_deployment(deployment.id, &deployment.domain)
                .await
            {
                Ok(_) => {
                    self.store
                        .deployments()
                        .update_dns_status(
                            deployment.id,
                            entity::sea_orm_active_enums::DnsStatus::Active,
                        )
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

        // Clean up DNS records whose deployment no longer exists. Only
        // considers records tracked in the local DB, leaving any records
        // not managed by Kennel untouched.
        let all_records = self.store.dns_records().find_all().await?;
        for record in all_records {
            let Some(deployment_id) = record.deployment_id else {
                continue;
            };
            let deployment = self
                .store
                .deployments()
                .find_by_id(deployment_id)
                .await
                .map_err(|e| crate::Error::Other(anyhow::anyhow!(e)))?;
            if deployment.is_none() {
                warn!("DNS record {} has no deployment, deleting", record.domain);
                if let Ok(provider) = self.get_provider_for_domain(&record.domain) {
                    if let Err(e) = provider.delete_record(&record.provider_record_id).await {
                        error!(
                            "Failed to delete orphaned DNS record {}: {}",
                            record.domain, e
                        );
                    }
                }
                self.store.dns_records().delete(record.id).await?;
                summary.dns_orphaned += 1;
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
