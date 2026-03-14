use std::path::Path;
use std::time::Duration;

use tracing::{info, warn};

use crate::error::Result;
use crate::{DeployerConfig, user, utils};

pub async fn process_teardown(
    deployment: entity::deployments::Model,
    config: &DeployerConfig,
) -> Result<()> {
    let deployment_id = deployment.id;
    info!("Tearing down deployment {deployment_id}");

    let process_name = utils::process_name(
        &deployment.project_name,
        &deployment.branch_slug,
        &deployment.service_name,
    );

    // Stop the supervised process
    if config.supervisor.is_running(&process_name).await {
        if let Err(e) = config
            .supervisor
            .stop(&process_name, Duration::from_secs(30))
            .await
        {
            warn!("Failed to stop process {process_name}: {e}");
        }
        config.supervisor.remove(&process_name).await;
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

    // Teardown provisioned resources.
    let resource_request = kennel_provision::ResourceRequest {
        project_name: deployment.project_name.clone(),
        service_name: deployment.service_name.clone(),
        branch: deployment.branch.clone(),
        branch_slug: deployment.branch_slug.clone(),
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

    let has_other_deployments = remaining_deployments.is_some_and(|d| d.id != deployment_id);
    if !has_other_deployments {
        let username = utils::sanitize_username(
            &deployment.project_name,
            &deployment.branch_slug,
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

    config
        .store
        .deployments()
        .delete(deployment_id)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!("Successfully tore down deployment {deployment_id}");

    Ok(())
}
