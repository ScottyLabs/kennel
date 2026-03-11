use std::path::PathBuf;

use entity::deployments;
use entity::sea_orm_active_enums::DeploymentStatus;
use kennel_config::parse_kennel_toml;
use kennel_supervisor::ProcessConfig;
use tracing::{error, info, warn};

use crate::error::Result;
use crate::{DeployerConfig, DeploymentRequest, static_site, user, utils};

pub(crate) fn determine_environment(git_ref: &str) -> String {
    match git_ref {
        "main" => "prod".to_string(),
        "staging" => "staging".to_string(),
        "dev" => "dev".to_string(),
        s if s.starts_with("pr-") => "preview".to_string(),
        _ => "dev".to_string(),
    }
}

pub async fn deploy_build(request: &DeploymentRequest, config: &DeployerConfig) -> Result<()> {
    let build_results = config
        .store
        .build_results()
        .find_successful_by_build_id(request.build_id)
        .await?;

    if build_results.is_empty() {
        warn!(
            "No successful build results found for build {}",
            request.build_id
        );
        return Ok(());
    }

    let _build = config
        .store
        .builds()
        .find_by_id(request.build_id)
        .await?
        .ok_or_else(|| crate::DeployerError::NotFound(format!("Build {}", request.build_id)))?;

    let work_dir = PathBuf::from(kennel_config::constants::DEFAULT_WORK_DIR)
        .join(request.build_id.to_string());
    let config_file = parse_kennel_toml(&work_dir).await.map_err(|e| {
        crate::DeployerError::Other(anyhow::anyhow!("Failed to parse kennel.toml: {}", e))
    })?;

    info!(
        "Deploying {} items for build {}",
        build_results.len(),
        request.build_id
    );

    // Deploy static sites from kennel.toml (not devenv processes).
    for build_result in &build_results {
        let is_static_site = config_file
            .static_sites
            .contains_key(&build_result.service_name);

        if is_static_site
            && let Err(e) =
                static_site::deploy_site(request, build_result, &config.store, config, &config_file)
                    .await
        {
            error!(
                "Failed to deploy static site '{}' from build {}: {}",
                build_result.service_name, request.build_id, e
            );
        }
    }

    // Deploy service processes from devenv task configs.
    for process_config in &request.process_configs {
        if let Err(e) = deploy_service(request, process_config, config, &config_file).await {
            error!(
                "Failed to deploy service '{}' from build {}: {}",
                process_config.name, request.build_id, e
            );
        }
    }

    Ok(())
}

async fn deploy_service(
    request: &DeploymentRequest,
    devenv_config: &ProcessConfig,
    config: &DeployerConfig,
    config_file: &kennel_config::KennelConfig,
) -> Result<()> {
    info!(
        "Deploying service '{}' from exec: {}",
        devenv_config.name, devenv_config.exec
    );

    let branch_sanitized = utils::sanitize_identifier(&request.git_ref);
    let process_name = format!(
        "kennel-{}-{}-{}",
        request.project_name, branch_sanitized, devenv_config.name
    );
    let username = utils::sanitize_username(
        &request.project_name,
        &branch_sanitized,
        &devenv_config.name,
    );

    user::ensure_user_exists(&username).await?;

    let service_work_dir = PathBuf::from(kennel_config::constants::SERVICES_BASE_DIR)
        .join(&request.project_name)
        .join(&branch_sanitized)
        .join(&devenv_config.name);
    tokio::fs::create_dir_all(&service_work_dir).await?;

    let mut process_config = merge_config(
        devenv_config,
        &process_name,
        &username,
        service_work_dir,
        &request.git_ref,
    );

    // Provision required infrastructure resources.
    let resource_request = kennel_provision::ResourceRequest {
        deployment_id: 0,
        project_name: request.project_name.clone(),
        service_name: devenv_config.name.clone(),
        branch: request.git_ref.clone(),
        branch_slug: branch_sanitized.clone(),
        environment: determine_environment(&request.git_ref),
        system_user: username.clone(),
    };

    for provider in &config.resource_providers {
        if request
            .required_resources
            .contains(&provider.name().to_string())
        {
            let env_vars = provider.provision(&resource_request).await.map_err(|e| {
                crate::DeployerError::Other(anyhow::anyhow!(
                    "resource provider '{}' failed: {e}",
                    provider.name()
                ))
            })?;
            process_config.env.extend(env_vars);
        }
    }

    let service_config = config_file.services.get(&devenv_config.name);

    // Resolve secrets from secretspec.toml if present.
    if let Some(vault_endpoint) = &config.vault_endpoint {
        let work_dir = PathBuf::from(kennel_config::constants::DEFAULT_WORK_DIR)
            .join(request.build_id.to_string())
            .join("repo");
        let environment = determine_environment(&request.git_ref);
        match kennel_secrets::resolve_secrets(&work_dir, &environment, vault_endpoint) {
            Ok(secrets) => {
                process_config.env.extend(secrets);
            }
            Err(e) => {
                warn!("Secret resolution failed: {e}");
            }
        }
    }

    let process_config_json = serde_json::to_value(&process_config)
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    let domain = utils::generate_deployment_domain(
        &devenv_config.name,
        &branch_sanitized,
        &request.project_name,
        &config.base_domain,
    );

    // Check for existing deployment (blue-green).
    let existing_deployment = config
        .store
        .deployments()
        .find_deployed_by_ref(&request.project_name, &request.git_ref, &devenv_config.name)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    if existing_deployment.is_some() {
        let mut supervisor = config.supervisor.lock().await;
        supervisor
            .blue_green_deploy(
                process_config.clone(),
                kennel_config::constants::BLUE_GREEN_DRAIN_TIMEOUT,
            )
            .await?;
    } else {
        let mut supervisor = config.supervisor.lock().await;
        supervisor.start(process_config.clone()).await?;
    }

    // Create deployment record.
    let deployment = deployments::ActiveModel {
        project_name: sea_orm::ActiveValue::Set(request.project_name.clone()),
        git_ref: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        service_name: sea_orm::ActiveValue::Set(devenv_config.name.clone()),
        branch: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        branch_slug: sea_orm::ActiveValue::Set(branch_sanitized.clone()),
        environment: sea_orm::ActiveValue::Set(determine_environment(&request.git_ref)),
        store_path: sea_orm::ActiveValue::Set(Some(devenv_config.exec.clone())),
        status: sea_orm::ActiveValue::Set(DeploymentStatus::Deployed),
        domain: sea_orm::ActiveValue::Set(domain.clone()),
        process_config: sea_orm::ActiveValue::Set(Some(process_config_json)),
        ..Default::default()
    };

    let new_deployment = config
        .store
        .deployments()
        .create(deployment)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!(
        "Successfully deployed service '{}' as {}",
        devenv_config.name, process_name
    );

    // Create DNS records for custom domain if configured.
    if let Some(dns_manager) = &config.dns_manager
        && let Some(custom_domain) = service_config.and_then(|s| s.custom_domain.as_ref())
    {
        info!("Creating DNS records for custom domain: {custom_domain}");
        match dns_manager
            .create_record_for_deployment(new_deployment.id, custom_domain)
            .await
        {
            Ok(_) => info!("DNS records created for {custom_domain}"),
            Err(e) => warn!("Failed to create DNS records for {custom_domain}: {e}"),
        }
    }

    // Mark old deployment as torn down.
    if let Some(old_deployment) = existing_deployment {
        let mut old_active = sea_orm::IntoActiveModel::into_active_model(old_deployment);
        old_active.status = sea_orm::ActiveValue::Set(DeploymentStatus::TornDown);
        if let Err(e) = config.store.deployments().update(old_active).await {
            error!("Failed to mark old deployment as torn down: {e}");
        }
    }

    Ok(())
}

/// Merge a devenv-provided ProcessConfig with Kennel deployment metadata.
/// Sets the process name, system user, working directory, and environment
/// for the deployment context.
fn merge_config(
    devenv_config: &ProcessConfig,
    process_name: &str,
    username: &str,
    cwd: PathBuf,
    git_ref: &str,
) -> ProcessConfig {
    let mut config = devenv_config.clone();
    config.name = process_name.to_string();
    config.user = Some(username.to_string());
    config.cwd = Some(cwd);
    config
        .env
        .insert("ENVIRONMENT".into(), determine_environment(git_ref));
    config
}
