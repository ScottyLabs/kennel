mod error;
mod health;
mod log_cleanup;
mod secrets;
mod service;
mod static_site;
mod systemd;
mod teardown;
mod user;
mod utils;

pub use error::{DeployerError, Result};
pub use kennel_builder::DeploymentRequest;
pub use log_cleanup::run_log_cleanup_job;
pub use teardown::run_teardown_worker;

use kennel_dns::DnsManager;
use kennel_router::RouterUpdate;
use kennel_store::Store;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Clone)]
pub struct DeployerConfig {
    pub store: Arc<Store>,
    pub router_tx: Option<tokio::sync::broadcast::Sender<RouterUpdate>>,
    pub dns_manager: Option<Arc<DnsManager>>,
    pub base_domain: String,
}

pub async fn run_deployer(
    mut deploy_rx: mpsc::Receiver<DeploymentRequest>,
    config: DeployerConfig,
) {
    info!("Starting deployer");

    while let Some(request) = deploy_rx.recv().await {
        info!(
            "Received deployment request for build {} (project: {}, ref: {})",
            request.build_id, request.project_name, request.git_ref
        );

        if let Err(e) = service::deploy_build(&request, &config).await {
            error!("Deployment failed for build {}: {}", request.build_id, e);
        }
    }

    info!("Deployer shutting down");
}

pub async fn run_cleanup_job(config: DeployerConfig, teardown_tx: mpsc::Sender<i32>) {
    info!("Starting auto-expiry cleanup job");

    let mut interval = tokio::time::interval(kennel_config::constants::CLEANUP_JOB_INTERVAL);

    loop {
        interval.tick().await;

        info!("Running auto-expiry cleanup");

        match config.store.find_expired_deployments(7).await {
            Ok(expired) if !expired.is_empty() => {
                let ids: Vec<i32> = expired.iter().map(|d| d.id).collect();

                for deployment in &expired {
                    info!(
                        "Auto-expiry: deployment {} (project: {}, ref: {}, last_activity: {:?})",
                        deployment.id,
                        deployment.project_name,
                        deployment.git_ref,
                        deployment.last_activity
                    );
                }

                if let Err(e) = config.store.deployments().mark_ids_tearing_down(&ids).await {
                    error!("Failed to mark deployments for teardown: {}", e);
                    continue;
                }

                for id in &ids {
                    if let Err(e) = teardown_tx.send(*id).await {
                        error!(
                            "Failed to send teardown request for deployment {}: {}",
                            id, e
                        );
                    }
                }

                info!(
                    "Marked {} deployment(s) for auto-expiry teardown",
                    ids.len()
                );
            }
            Err(e) => {
                error!("Cleanup job failed to find expired deployments: {}", e);
            }
            _ => {}
        }
    }
}
