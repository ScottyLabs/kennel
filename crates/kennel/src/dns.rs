use kennel_config::constants;
use kennel_store::Store;
use std::sync::Arc;
use tracing::{info, warn};

pub async fn initialize_dns(
    store: Arc<Store>,
    base_domain: &str,
) -> anyhow::Result<Option<Arc<kennel_dns::DnsManager>>> {
    if !std::env::var("DNS_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let server_ipv4 = std::env::var("DNS_SERVER_IPV4")
        .expect("DNS_SERVER_IPV4 must be set when DNS is enabled")
        .parse()
        .expect("DNS_SERVER_IPV4 must be a valid IPv4 address");
    let server_ipv6 = std::env::var("DNS_SERVER_IPV6")
        .expect("DNS_SERVER_IPV6 must be set when DNS is enabled")
        .parse()
        .expect("DNS_SERVER_IPV6 must be a valid IPv6 address");

    let cloudflare_zones_json = std::env::var("DNS_CLOUDFLARE_ZONES")
        .expect("DNS_CLOUDFLARE_ZONES must be set when DNS is enabled");
    let cloudflare_zones: std::collections::HashMap<String, String> =
        serde_json::from_str(&cloudflare_zones_json)
            .expect("DNS_CLOUDFLARE_ZONES must be valid JSON");

    let cloudflare_api_token = std::env::var("CLOUDFLARE_API_TOKEN")
        .expect("CLOUDFLARE_API_TOKEN must be set when DNS is enabled");

    let providers: std::collections::HashMap<String, Arc<dyn kennel_dns::DnsProvider>> =
        cloudflare_zones
            .into_iter()
            .map(|(zone_name, zone_id)| {
                let provider =
                    kennel_dns::CloudflareProvider::new(cloudflare_api_token.clone(), zone_id)?;
                Ok((
                    zone_name,
                    Arc::new(provider) as Arc<dyn kennel_dns::DnsProvider>,
                ))
            })
            .collect::<anyhow::Result<_>>()?;

    let dns_manager =
        kennel_dns::DnsManager::new(providers, store.clone(), server_ipv4, server_ipv6);
    let dns_manager = Arc::new(dns_manager);

    reconcile_dns(&dns_manager).await;
    create_wildcard_dns(&dns_manager, base_domain).await;

    Ok(Some(dns_manager))
}

async fn reconcile_dns(dns_manager: &kennel_dns::DnsManager) {
    info!("Running DNS reconciliation");
    match dns_manager.reconcile().await {
        Ok(summary) => {
            info!(
                "DNS reconciliation complete: {} created, {} failed, {} orphaned",
                summary.dns_created, summary.dns_failed, summary.dns_orphaned
            );
        }
        Err(e) => {
            warn!("DNS reconciliation failed: {}", e);
        }
    }
}

async fn create_wildcard_dns(dns_manager: &kennel_dns::DnsManager, base_domain: &str) {
    match tokio::fs::read_to_string(constants::PROJECTS_CONFIG_PATH).await {
        Ok(projects_json) => match serde_json::from_str::<Vec<serde_json::Value>>(&projects_json) {
            Ok(projects) => {
                for project in projects.iter().filter_map(|p| p.get("name")?.as_str()) {
                    info!("Creating wildcard DNS for project: {}", project);
                    if let Err(e) = dns_manager
                        .create_wildcard_for_project(project, base_domain)
                        .await
                    {
                        warn!(
                            "Failed to create wildcard DNS for project {}: {}",
                            project, e
                        );
                    }
                }
            }
            Err(e) => warn!("Failed to parse projects.json: {}", e),
        },
        Err(_) => info!("No projects.json found, skipping wildcard DNS creation"),
    }
}
