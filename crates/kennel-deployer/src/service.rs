use std::path::PathBuf;

use entity::deployments;
use entity::sea_orm_active_enums::DeploymentStatus;
use kennel_supervisor::ProcessConfig;
use tracing::{error, info, warn};

use crate::error::Result;
use crate::{DeployerConfig, static_site, user, utils};

pub(crate) fn determine_environment(git_ref: &str) -> entity::sea_orm_active_enums::Environment {
    use entity::sea_orm_active_enums::Environment;
    match git_ref {
        "main" => Environment::Prod,
        "staging" => Environment::Staging,
        "dev" => Environment::Dev,
        s if s.starts_with("pr-") => Environment::Preview,
        _ => Environment::Dev,
    }
}

/// Deploy a completed build. Reads process_configs, required_resources, and
/// kennel_config from the build record's JSONB columns.
pub async fn deploy_build(build: &entity::builds::Model, config: &DeployerConfig) -> Result<()> {
    let build_results = config
        .store
        .build_results()
        .find_successful_by_build_id(build.id)
        .await?;

    if build_results.is_empty() {
        warn!("No successful build results found for build {}", build.id);
        return Ok(());
    }

    // Deserialize build outputs from JSONB columns.
    let process_configs: Vec<ProcessConfig> = build
        .process_configs
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let required_resources: Vec<String> = build
        .required_resources
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let kennel_config: kennel_config::KennelConfig = build
        .kennel_config
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| kennel_config::KennelConfig {
            services: Default::default(),
            static_sites: Default::default(),
        });

    info!(
        "Deploying {} items for build {}",
        build_results.len(),
        build.id
    );

    // Deploy static sites.
    for build_result in &build_results {
        if kennel_config
            .static_sites
            .contains_key(&build_result.service_name)
        {
            if let Err(e) =
                static_site::deploy_site(build, build_result, &config.store, config, &kennel_config)
                    .await
            {
                error!(
                    "Failed to deploy static site '{}' from build {}: {}",
                    build_result.service_name, build.id, e
                );
            }
        }
    }

    // Deploy service processes.
    for process_config in &process_configs {
        if let Err(e) = deploy_service(
            build,
            process_config,
            &required_resources,
            config,
            &kennel_config,
        )
        .await
        {
            error!(
                "Failed to deploy service '{}' from build {}: {}",
                process_config.name, build.id, e
            );
        }
    }

    Ok(())
}

async fn deploy_service(
    build: &entity::builds::Model,
    devenv_config: &ProcessConfig,
    required_resources: &[String],
    config: &DeployerConfig,
    config_file: &kennel_config::KennelConfig,
) -> Result<()> {
    info!(
        "Deploying service '{}' from exec: {}",
        devenv_config.name, devenv_config.exec
    );

    let branch = &build.branch;
    let branch_sanitized = utils::sanitize_identifier(branch);
    let process_name =
        utils::process_name(&build.project_name, &branch_sanitized, &devenv_config.name);
    let username =
        utils::sanitize_username(&build.project_name, &branch_sanitized, &devenv_config.name);

    user::ensure_user_exists(&username).await?;

    let service_work_dir = PathBuf::from(kennel_config::constants::SERVICES_BASE_DIR)
        .join(&build.project_name)
        .join(&branch_sanitized)
        .join(&devenv_config.name);
    tokio::fs::create_dir_all(&service_work_dir).await?;

    let environment = determine_environment(branch);
    let mut process_config = merge_config(
        devenv_config,
        &process_name,
        &username,
        service_work_dir,
        &environment,
    );

    // Provision required infrastructure resources.
    let resource_request = kennel_provision::ResourceRequest {
        project_name: build.project_name.clone(),
        service_name: devenv_config.name.clone(),
        branch: branch.clone(),
        branch_slug: branch_sanitized.clone(),
        environment: format!("{:?}", environment).to_lowercase(),
        system_user: username.clone(),
    };

    for provider in &config.resource_providers {
        if required_resources.contains(&provider.name().to_string()) {
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
            .join(build.id.to_string())
            .join("repo");
        let env_str = format!("{:?}", environment).to_lowercase();
        match crate::secrets::resolve_secrets(&work_dir, &env_str, vault_endpoint) {
            Ok(secrets) => {
                process_config.env.extend(secrets);
            }
            Err(e) => {
                warn!("Secret resolution failed: {e}");
            }
        }
    }

    // Strip secrets from the process config before persisting to DB.
    // Secrets are resolved at deploy time and only live in memory.
    let process_config_for_db = process_config.clone();
    // TODO: identify and strip secret keys from process_config_for_db.env
    let process_config_json = serde_json::to_value(&process_config_for_db)
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    let domain = utils::generate_deployment_domain(
        &devenv_config.name,
        &branch_sanitized,
        &build.project_name,
        &config.base_domain,
    );

    // Check for existing deployment (blue-green vs fresh deploy).
    let existing_deployment = config
        .store
        .deployments()
        .find_deployed_by_ref(&build.project_name, branch, &devenv_config.name)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    let deployment_id = if let Some(existing) = &existing_deployment {
        // Update existing deployment to Deploying with new config.
        let mut active = sea_orm::IntoActiveModel::into_active_model(existing.clone());
        active.status = sea_orm::ActiveValue::Set(DeploymentStatus::Deploying);
        active.store_path = sea_orm::ActiveValue::Set(Some(devenv_config.exec.clone()));
        active.process_config = sea_orm::ActiveValue::Set(Some(process_config_json));
        active.process_name = sea_orm::ActiveValue::Set(Some(process_name.clone()));
        active.git_ref = sea_orm::ActiveValue::Set(build.git_ref.clone());
        active.updated_at = sea_orm::ActiveValue::Set(chrono::Utc::now().naive_utc());
        let updated = config
            .store
            .deployments()
            .update(active)
            .await
            .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;
        updated.id
    } else {
        // Create new deployment record with Deploying status.
        let deployment = deployments::ActiveModel {
            project_name: sea_orm::ActiveValue::Set(build.project_name.clone()),
            git_ref: sea_orm::ActiveValue::Set(build.git_ref.clone()),
            service_name: sea_orm::ActiveValue::Set(devenv_config.name.clone()),
            branch: sea_orm::ActiveValue::Set(branch.clone()),
            branch_slug: sea_orm::ActiveValue::Set(branch_sanitized.clone()),
            environment: sea_orm::ActiveValue::Set(environment.clone()),
            store_path: sea_orm::ActiveValue::Set(Some(devenv_config.exec.clone())),
            status: sea_orm::ActiveValue::Set(DeploymentStatus::Deploying),
            domain: sea_orm::ActiveValue::Set(domain.clone()),
            process_config: sea_orm::ActiveValue::Set(Some(process_config_json)),
            process_name: sea_orm::ActiveValue::Set(Some(process_name.clone())),
            ..Default::default()
        };
        let new = config
            .store
            .deployments()
            .create(deployment)
            .await
            .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;
        new.id
    };

    if existing_deployment.is_some() {
        config
            .supervisor
            .blue_green_deploy(
                process_config,
                kennel_config::constants::BLUE_GREEN_DRAIN_TIMEOUT,
            )
            .await?;
    } else {
        config.supervisor.start(process_config).await?;
    }

    // Mark deployment as Deployed.
    let active = deployments::ActiveModel {
        id: sea_orm::ActiveValue::Set(deployment_id),
        status: sea_orm::ActiveValue::Set(DeploymentStatus::Deployed),
        updated_at: sea_orm::ActiveValue::Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    config
        .store
        .deployments()
        .update(active)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!(
        "Successfully deployed service '{}' as {}",
        devenv_config.name, process_name
    );

    if let Some(dns_manager) = &config.dns_manager
        && let Some(custom_domain) = service_config.and_then(|s| s.custom_domain.as_ref())
    {
        utils::create_custom_domain_dns(dns_manager, &config.store, deployment_id, custom_domain)
            .await;
    }

    Ok(())
}

/// Merge a devenv-provided ProcessConfig with Kennel deployment metadata.
fn merge_config(
    devenv_config: &ProcessConfig,
    process_name: &str,
    username: &str,
    cwd: PathBuf,
    environment: &entity::sea_orm_active_enums::Environment,
) -> ProcessConfig {
    let env_str = match environment {
        entity::sea_orm_active_enums::Environment::Prod => "prod",
        entity::sea_orm_active_enums::Environment::Staging => "staging",
        entity::sea_orm_active_enums::Environment::Dev => "dev",
        entity::sea_orm_active_enums::Environment::Preview => "preview",
    };
    let mut config = devenv_config.clone();
    config.name = process_name.to_string();
    config.user = Some(username.to_string());
    config.cwd = Some(cwd);
    config.env.insert("ENVIRONMENT".into(), env_str.to_string());
    config
}
