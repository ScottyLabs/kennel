use crate::error::Result;
use crate::{DeployerConfig, DeploymentRequest, utils};
use entity::sea_orm_active_enums::DeploymentStatus;
use entity::{build_results, deployments};
use kennel_config::KennelConfig;
use kennel_store::Store;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

pub async fn deploy_site(
    request: &DeploymentRequest,
    build_result: &build_results::Model,
    store: &Arc<Store>,
    config: &DeployerConfig,
    kennel_config: &KennelConfig,
) -> Result<()> {
    let store_path = build_result
        .store_path
        .as_ref()
        .ok_or_else(|| crate::DeployerError::Other(anyhow::anyhow!("No store path")))?;

    info!(
        "Deploying static site '{}' from store path: {}",
        build_result.service_name, store_path
    );

    let branch_sanitized = utils::sanitize_identifier(&request.git_ref);
    let site_base_dir = PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
        .join(&request.project_name)
        .join(&branch_sanitized);

    tokio::fs::create_dir_all(&site_base_dir).await?;

    let site_link = site_base_dir.join(&build_result.service_name);
    let temp_link = site_base_dir.join(format!("{}.new", build_result.service_name));

    if temp_link.exists() {
        tokio::fs::remove_file(&temp_link).await?;
    }

    #[cfg(unix)]
    tokio::fs::symlink(store_path, &temp_link).await?;

    #[cfg(not(unix))]
    {
        return Err(crate::DeployerError::Other(anyhow::anyhow!(
            "Symlinks only supported on Unix systems"
        )));
    }

    if site_link.exists() {
        tokio::fs::remove_file(&site_link).await?;
    }

    tokio::fs::rename(&temp_link, &site_link).await?;

    let deployment = deployments::ActiveModel {
        project_name: sea_orm::ActiveValue::Set(request.project_name.clone()),
        git_ref: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        service_name: sea_orm::ActiveValue::Set(build_result.service_name.clone()),
        branch: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        branch_slug: sea_orm::ActiveValue::Set(branch_sanitized.clone()),
        environment: sea_orm::ActiveValue::Set(crate::service::determine_environment(
            &request.git_ref,
        )),
        store_path: sea_orm::ActiveValue::Set(Some(store_path.clone())),
        port: sea_orm::ActiveValue::Set(None),
        status: sea_orm::ActiveValue::Set(DeploymentStatus::Active),
        domain: sea_orm::ActiveValue::Set(utils::generate_deployment_domain(
            &build_result.service_name,
            &branch_sanitized,
            &request.project_name,
            &config.base_domain,
        )),
        ..Default::default()
    };

    let new_deployment = store
        .deployments()
        .create(deployment)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!(
        "Successfully deployed static site '{}' to {}",
        build_result.service_name,
        site_link.display()
    );

    // Create DNS records for custom domain if configured
    let site_config = kennel_config.static_sites.get(&build_result.service_name);
    if let Some(dns_manager) = &config.dns_manager
        && let Some(custom_domain) = site_config.and_then(|s| s.custom_domain.as_ref())
    {
        info!("Creating DNS records for custom domain: {}", custom_domain);
        match dns_manager
            .create_record_for_deployment(new_deployment.id, custom_domain)
            .await
        {
            Ok(_) => {
                info!(
                    "DNS records created successfully for custom domain {}",
                    custom_domain
                );
            }
            Err(e) => {
                warn!(
                    "Failed to create DNS records for custom domain {}: {}",
                    custom_domain, e
                );
            }
        }
    }

    // Notify router of new deployment
    if let Some(ref router_tx) = config.router_tx {
        let update = kennel_router::RouterUpdate::DeploymentActive {
            deployment_id: new_deployment.id,
            domain: new_deployment.domain.clone(),
            port: None,
            store_path: Some(store_path.clone()),
            spa: site_config.map(|s| s.spa).unwrap_or(false),
        };

        if let Err(e) = router_tx.send(update) {
            warn!("Failed to send router update: {}", e);
        }
    }

    Ok(())
}
