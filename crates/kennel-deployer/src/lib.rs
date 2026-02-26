mod error;
mod health;
mod ports;
mod secrets;
mod service;
mod static_site;
mod systemd;
mod teardown;
mod user;

pub use error::{DeployerError, Result};
pub use kennel_builder::DeploymentRequest;
pub use ports::PortAllocator;
pub use teardown::{TeardownRequest, run_teardown_worker};

use kennel_router::RouterUpdate;
use kennel_store::Store;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Clone)]
pub struct DeployerConfig {
    pub store: Arc<Store>,
    pub port_allocator: Arc<PortAllocator>,
    pub router_tx: Option<tokio::sync::broadcast::Sender<RouterUpdate>>,
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

pub async fn run_cleanup_job(config: DeployerConfig, teardown_tx: mpsc::Sender<TeardownRequest>) {
    info!("Starting auto-expiry cleanup job");

    let mut interval = tokio::time::interval(kennel_config::constants::CLEANUP_JOB_INTERVAL);

    loop {
        interval.tick().await;

        info!("Running auto-expiry cleanup");

        match config.store.find_expired_deployments(7).await {
            Ok(expired) => {
                for deployment in &expired {
                    info!(
                        "Auto-expiry: marking deployment {} for teardown (project: {}, ref: {}, last_activity: {:?})",
                        deployment.id,
                        deployment.project_name,
                        deployment.git_ref,
                        deployment.last_activity
                    );

                    if let Err(e) = config
                        .store
                        .deployments()
                        .update({
                            use entity::sea_orm_active_enums::DeploymentStatus;
                            use sea_orm::{ActiveValue::Set, IntoActiveModel};

                            let mut active = deployment.clone().into_active_model();
                            active.status = Set(DeploymentStatus::TearingDown);
                            active
                        })
                        .await
                    {
                        error!(
                            "Failed to mark deployment {} for teardown: {}",
                            deployment.id, e
                        );
                        continue;
                    }

                    if let Err(e) = teardown_tx
                        .send(TeardownRequest {
                            deployment_id: deployment.id,
                        })
                        .await
                    {
                        error!(
                            "Failed to send teardown request for deployment {}: {}",
                            deployment.id, e
                        );
                    }
                }

                if !expired.is_empty() {
                    info!(
                        "Marked {} deployment(s) for auto-expiry teardown",
                        expired.len()
                    );
                }
            }
            Err(e) => {
                error!("Cleanup job failed to find expired deployments: {}", e);
            }
        }
    }
}
