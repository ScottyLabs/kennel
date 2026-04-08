use std::path::PathBuf;
use std::sync::Arc;

use entity::deployments;
use entity::sea_orm_active_enums::DeploymentStatus;
use kennel_config::KennelConfig;
use kennel_store::Store;
use kennel_supervisor::SupervisorEvent;
use tracing::info;

use crate::error::Result;
use crate::{DeployerConfig, utils};

pub async fn deploy_site(
    build: &entity::builds::Model,
    build_result: &entity::build_results::Model,
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

    let branch_sanitized = utils::sanitize_identifier(&build.branch);
    let site_base_dir = PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
        .join(&build.project_name)
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

    let domain = utils::generate_deployment_domain(
        &build_result.service_name,
        &branch_sanitized,
        &build.project_name,
        &config.base_domain,
    );

    let deployment = deployments::ActiveModel {
        project_id: sea_orm::ActiveValue::Set(build.project_id),
        project_name: sea_orm::ActiveValue::Set(build.project_name.clone()),
        git_ref: sea_orm::ActiveValue::Set(build.git_ref.clone()),
        service_name: sea_orm::ActiveValue::Set(build_result.service_name.clone()),
        branch: sea_orm::ActiveValue::Set(build.branch.clone()),
        branch_slug: sea_orm::ActiveValue::Set(branch_sanitized.clone()),
        environment: sea_orm::ActiveValue::Set(crate::service::determine_environment(
            &build.branch,
        )),
        store_path: sea_orm::ActiveValue::Set(Some(store_path.clone())),
        status: sea_orm::ActiveValue::Set(DeploymentStatus::Deployed),
        domain: sea_orm::ActiveValue::Set(domain.clone()),
        process_config: sea_orm::ActiveValue::Set(None),
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

    let site_config = kennel_config.static_sites.get(&build_result.service_name);
    if let Some(dns_manager) = &config.dns_manager
        && let Some(custom_domain) = site_config.and_then(|s| s.custom_domain.as_ref())
    {
        crate::utils::create_custom_domain_dns(
            dns_manager,
            store,
            new_deployment.id,
            custom_domain,
        )
        .await;
    }

    let _ = config
        .supervisor
        .event_sender()
        .send(SupervisorEvent::ProcessReady {
            name: utils::process_name(
                &build.project_name,
                &branch_sanitized,
                &build_result.service_name,
            ),
            port: None,
            store_path: Some(store_path.clone()),
        });

    Ok(())
}
