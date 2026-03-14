mod error;
mod log_cleanup;
mod secrets;
mod service;
mod static_site;
mod teardown;
mod user;
mod utils;

pub use error::{DeployerError, Result};
pub use log_cleanup::run_log_cleanup_job;

use kennel_dns::DnsManager;
use kennel_provision::ResourceProvider;
use kennel_store::Store;
use kennel_supervisor::SupervisorHandle;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Clone)]
pub struct DeployerConfig {
    pub store: Arc<Store>,
    pub supervisor: SupervisorHandle,
    pub dns_manager: Option<Arc<DnsManager>>,
    pub resource_providers: Vec<Arc<dyn ResourceProvider>>,
    pub vault_endpoint: Option<String>,
    pub base_domain: String,
}

pub async fn run_deployer(
    config: DeployerConfig,
    deploy_signal: Arc<Notify>,
    cancel: CancellationToken,
) {
    info!("Starting deployer");

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let build = match config.store.builds().claim_built_build().await {
            Ok(Some(build)) => build,
            Ok(None) => {
                tokio::select! {
                    _ = deploy_signal.notified() => continue,
                    _ = cancel.cancelled() => break,
                }
            }
            Err(e) => {
                error!("Failed to claim built build: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let build_id = build.id;
        info!(
            "Deploying build {build_id} (project: {}, ref: {})",
            build.project_name, build.git_ref
        );

        if let Err(e) = service::deploy_build(&build, &config).await {
            error!("Deployment failed for build {build_id}: {e}");
            if let Err(e2) = config.store.builds().mark_failed(build_id).await {
                error!("Failed to mark build {build_id} as failed: {e2}");
            }
        } else {
            if let Err(e) = config.store.builds().mark_success(build_id).await {
                error!("Failed to mark build {build_id} as success: {e}");
            }
        }
    }

    info!("Deployer shutting down");
}

pub async fn run_teardown_worker(
    config: DeployerConfig,
    teardown_signal: Arc<Notify>,
    cancel: CancellationToken,
) {
    info!("Starting teardown worker");

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let deployment = match config.store.deployments().claim_tearing_down().await {
            Ok(Some(d)) => d,
            Ok(None) => {
                tokio::select! {
                    _ = teardown_signal.notified() => continue,
                    _ = cancel.cancelled() => break,
                }
            }
            Err(e) => {
                error!("Failed to claim teardown deployment: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let deployment_id = deployment.id;
        info!("Processing teardown for deployment {deployment_id}");

        if let Err(e) = teardown::process_teardown(deployment, &config).await {
            error!("Teardown failed for deployment {deployment_id}: {e}");
        }
    }

    info!("Teardown worker shutting down");
}

pub async fn run_cleanup_job(
    config: DeployerConfig,
    teardown_signal: Arc<Notify>,
    cancel: CancellationToken,
) {
    info!("Starting auto-expiry cleanup job");

    let mut interval = tokio::time::interval(kennel_config::constants::CLEANUP_JOB_INTERVAL);

    loop {
        tokio::select! {
            _ = interval.tick() => {},
            _ = cancel.cancelled() => break,
        }

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

                if let Err(e) = config.store.deployments().mark_tearing_down(&ids).await {
                    error!("Failed to mark deployments for teardown: {e}");
                    continue;
                }

                teardown_signal.notify_one();

                info!(
                    "Marked {} deployment(s) for auto-expiry teardown",
                    ids.len()
                );
            }
            Err(e) => {
                error!("Cleanup job failed to find expired deployments: {e}");
            }
            _ => {}
        }
    }

    info!("Cleanup job shutting down");
}
