use crate::error::Result;
use crate::{DeployerConfig, secrets, systemd, user};
use entity::sea_orm_active_enums::DeploymentStatus;
use sea_orm::{ActiveValue::Set, IntoActiveModel};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct TeardownRequest {
    pub deployment_id: i32,
}

pub async fn run_teardown_worker(
    mut teardown_rx: mpsc::Receiver<TeardownRequest>,
    config: DeployerConfig,
) {
    info!("Starting teardown worker");

    while let Some(request) = teardown_rx.recv().await {
        info!(
            "Processing teardown request for deployment {}",
            request.deployment_id
        );

        if let Err(e) = process_teardown(&request, &config).await {
            error!(
                "Teardown failed for deployment {}: {}",
                request.deployment_id, e
            );
        }
    }

    info!("Teardown worker shutting down");
}

async fn process_teardown(request: &TeardownRequest, config: &DeployerConfig) -> Result<()> {
    let deployment = config
        .store
        .deployments()
        .find_by_id(request.deployment_id)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?
        .ok_or_else(|| {
            crate::DeployerError::NotFound(format!("Deployment {}", request.deployment_id))
        })?;

    if deployment.status != DeploymentStatus::TearingDown {
        warn!(
            "Deployment {} is not in TearingDown status, skipping",
            request.deployment_id
        );
        return Ok(());
    }

    info!("Tearing down deployment {}", request.deployment_id);

    let branch_sanitized = deployment.branch_slug.clone();

    // Stop systemd service if it's a service deployment
    if deployment.port.is_some() {
        let unit_name = format!(
            "kennel-{}-{}-{}",
            deployment.project_name, branch_sanitized, deployment.service_name
        );

        info!("Stopping systemd unit: {}", unit_name);

        if let Err(e) = systemd::stop_unit(&unit_name).await {
            warn!("Failed to stop unit {}: {}", unit_name, e);
        }

        if let Err(e) = systemd::disable_unit(&unit_name).await {
            warn!("Failed to disable unit {}: {}", unit_name, e);
        }

        if let Err(e) = systemd::remove_unit(&unit_name).await {
            warn!("Failed to remove unit {}: {}", unit_name, e);
        }

        if let Err(e) = systemd::daemon_reload().await {
            warn!("Failed to reload systemd daemon: {}", e);
        }

        // Release port
        if let Some(port) = deployment.port {
            config.port_allocator.release(port as u16).await;
            info!("Released port {}", port);
        }
    }

    // Remove static symlink if it's a static deployment (port is None for static sites)
    if deployment.port.is_none() {
        let static_link_path = format!(
            "{}/{}/{}/{}",
            kennel_config::constants::SITES_BASE_DIR,
            deployment.project_name,
            deployment.branch_slug,
            deployment.service_name
        );
        let static_link = Path::new(&static_link_path);
        if let Err(e) = tokio::fs::remove_file(static_link).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove static symlink {:?}: {}", static_link, e);
            }
        } else {
            info!("Removed static symlink: {:?}", static_link);
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
        warn!("Failed to remove secrets file: {}", e);
    }

    // Release preview database if this was the last deployment for this branch
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
        // No more deployments for this branch, release preview database
        if let Err(e) = config
            .store
            .preview_databases()
            .delete_by_project_and_branch(&deployment.project_name, &deployment.branch)
            .await
        {
            warn!("Failed to release preview database: {}", e);
        } else {
            info!(
                "Released preview database for {}/{}",
                deployment.project_name, deployment.branch
            );
        }

        // No more deployments for this project+branch+service, remove system user
        let username = user::sanitize_username(
            &deployment.project_name,
            &deployment.branch,
            &deployment.service_name,
        );

        if let Err(e) = user::remove_user(&username).await {
            warn!("Failed to remove system user {}: {}", username, e);
        } else {
            info!("Removed system user: {}", username);
        }
    }

    // Mark as torn down
    let mut deployment_active = deployment.into_active_model();
    deployment_active.status = Set(DeploymentStatus::TornDown);
    config
        .store
        .deployments()
        .update(deployment_active)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    // Delete deployment record
    config
        .store
        .deployments()
        .delete(request.deployment_id)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!(
        "Successfully tore down deployment {}",
        request.deployment_id
    );

    Ok(())
}
