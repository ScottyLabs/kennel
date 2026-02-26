use crate::table::RoutingTable;
use kennel_store::Store;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
struct HealthStatus {
    consecutive_failures: u32,
    is_healthy: bool,
}

pub async fn run_health_monitor(table: Arc<RoutingTable>, store: Arc<Store>) {
    info!("Starting health monitor");

    let health_status: Arc<RwLock<HashMap<String, HealthStatus>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let mut interval = time::interval(kennel_config::constants::HEALTH_CHECK_INTERVAL);

    loop {
        interval.tick().await;

        debug!("Running health checks");

        // Get all active service deployments
        match store.deployments().list_active().await {
            Ok(deployments) => {
                for deployment in deployments {
                    // Only check service deployments, not static sites
                    if let Some(port) = deployment.port {
                        let health_url = format!("http://localhost:{}/health", port);

                        let is_healthy = match tokio::time::timeout(
                            kennel_config::constants::HEALTH_CHECK_TIMEOUT,
                            reqwest::get(&health_url),
                        )
                        .await
                        {
                            Ok(Ok(response)) if response.status().is_success() => true,
                            Ok(Ok(response)) => {
                                warn!(
                                    "Health check failed for deployment {} ({}): HTTP {}",
                                    deployment.id,
                                    deployment.domain,
                                    response.status()
                                );
                                false
                            }
                            Ok(Err(e)) => {
                                warn!(
                                    "Health check failed for deployment {} ({}): {}",
                                    deployment.id, deployment.domain, e
                                );
                                false
                            }
                            Err(_) => {
                                warn!(
                                    "Health check timeout for deployment {} ({})",
                                    deployment.id, deployment.domain
                                );
                                false
                            }
                        };

                        let mut status_map = health_status.write().await;
                        let status =
                            status_map
                                .entry(deployment.domain.clone())
                                .or_insert(HealthStatus {
                                    consecutive_failures: 0,
                                    is_healthy: true,
                                });

                        if is_healthy {
                            // Reset failures on success
                            if status.consecutive_failures > 0 {
                                info!(
                                    "Deployment {} ({}) recovered",
                                    deployment.id, deployment.domain
                                );
                            }
                            status.consecutive_failures = 0;
                            status.is_healthy = true;
                        } else {
                            status.consecutive_failures += 1;

                            if status.consecutive_failures
                                >= kennel_config::constants::MAX_CONSECUTIVE_HEALTH_FAILURES
                                && status.is_healthy
                            {
                                error!(
                                    "Deployment {} ({}) failed {} consecutive health checks, removing from routing table",
                                    deployment.id, deployment.domain, status.consecutive_failures
                                );

                                // Remove from routing table
                                table.remove(&deployment.domain).await;
                                status.is_healthy = false;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to query deployments for health check: {}", e);
            }
        }
    }
}
