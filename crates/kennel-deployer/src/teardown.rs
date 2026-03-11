use std::path::{Path, PathBuf};
use std::time::Duration;

use entity::sea_orm_active_enums::DeploymentStatus;
use sea_orm::{ActiveValue::Set, IntoActiveModel};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::error::Result;
use crate::{DeployerConfig, secrets, user, utils};

pub async fn run_teardown_worker(mut teardown_rx: mpsc::Receiver<i32>, config: DeployerConfig) {
    info!("Starting teardown worker");

    while let Some(deployment_id) = teardown_rx.recv().await {
        info!("Processing teardown request for deployment {deployment_id}");

        if let Err(e) = process_teardown(deployment_id, &config).await {
            error!("Teardown failed for deployment {deployment_id}: {e}");
        }
    }

    info!("Teardown worker shutting down");
}

async fn process_teardown(deployment_id: i32, config: &DeployerConfig) -> Result<()> {
    let deployment = config
        .store
        .deployments()
        .find_by_id(deployment_id)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?
        .ok_or_else(|| crate::DeployerError::NotFound(format!("Deployment {deployment_id}")))?;

    info!("Tearing down deployment {deployment_id}");

    let branch_sanitized = deployment.branch_slug.clone();
    let process_name = format!(
        "kennel-{}-{}-{}",
        deployment.project_name, branch_sanitized, deployment.service_name
    );

    // Stop the supervised process
    {
        let mut supervisor = config.supervisor.lock().await;
        if supervisor.is_running(&process_name) {
            if let Err(e) = supervisor
                .stop(&process_name, Duration::from_secs(30))
                .await
            {
                warn!("Failed to stop process {process_name}: {e}");
            }
            supervisor.remove(&process_name);
        }
    }

    // Remove static symlink if it's a static site deployment
    let static_link_path = format!(
        "{}/{}/{}/{}",
        kennel_config::constants::SITES_BASE_DIR,
        deployment.project_name,
        deployment.branch_slug,
        deployment.service_name
    );
    let static_link = Path::new(&static_link_path);
    if static_link.exists() {
        if let Err(e) = tokio::fs::remove_file(static_link).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove static symlink {static_link_path}: {e}");
            }
        } else {
            info!("Removed static symlink: {static_link_path}");
        }
    }

    // Remove secrets file
    let secrets_path = PathBuf::from(format!(
        "{}/{}-{}-{}.env",
        kennel_config::constants::SECRETS_DIR,
        deployment.project_name,
        branch_sanitized,
        deployment.service_name
    ));
    if let Err(e) = secrets::remove_secrets_file(&secrets_path).await {
        warn!("Failed to remove secrets file: {e}");
    }

    // Teardown provisioned resources.
    let resource_request = kennel_provision::ResourceRequest {
        deployment_id: deployment.id,
        project_name: deployment.project_name.clone(),
        service_name: deployment.service_name.clone(),
        branch: deployment.branch.clone(),
        branch_slug: branch_sanitized.clone(),
        environment: deployment.environment.clone(),
        system_user: process_name.clone(),
    };

    for provider in &config.resource_providers {
        if let Err(e) = provider.teardown(&resource_request).await {
            warn!(
                "Resource provider '{}' teardown failed: {e}",
                provider.name()
            );
        }
    }

    // Remove system user if no remaining deployments for this branch.
    let remaining_deployments = config
        .store
        .deployments()
        .find_by_project_service_branch(
            &deployment.project_name,
            &deployment.service_name,
            &deployment.branch,
        )
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    if remaining_deployments.is_none() {
        let username = utils::sanitize_username(
            &deployment.project_name,
            &deployment.branch,
            &deployment.service_name,
        );
        if let Err(e) = user::remove_user(&username).await {
            warn!("Failed to remove system user {username}: {e}");
        } else {
            info!("Removed system user: {username}");
        }
    }

    // Delete DNS records
    if let Some(dns_manager) = &config.dns_manager
        && let Err(e) = dns_manager
            .delete_record_for_deployment(deployment_id)
            .await
    {
        warn!("Failed to delete DNS records: {e}");
    }

    // Mark as torn down and delete
    let mut deployment_active = deployment.into_active_model();
    deployment_active.status = Set(DeploymentStatus::TornDown);
    config
        .store
        .deployments()
        .update(deployment_active)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    config
        .store
        .deployments()
        .delete(deployment_id)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!("Successfully tore down deployment {deployment_id}");

    Ok(())
}
