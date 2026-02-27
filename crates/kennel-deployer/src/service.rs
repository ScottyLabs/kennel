use crate::error::Result;
use crate::{
    DeployerConfig, DeploymentRequest, health, secrets, static_site, systemd, user, utils,
};
use entity::sea_orm_active_enums::DeploymentStatus;
use entity::{build_results, deployments};
use kennel_config::parse_kennel_toml;
use sea_orm::IntoActiveModel;
use std::path::PathBuf;
use tracing::{error, info, warn};

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

    for build_result in build_results {
        let is_static_site = config_file
            .static_sites
            .contains_key(&build_result.service_name);

        if is_static_site {
            if let Err(e) = static_site::deploy_site(
                request,
                &build_result,
                &config.store,
                config,
                &config_file,
            )
            .await
            {
                error!(
                    "Failed to deploy static site '{}' from build {}: {}",
                    build_result.service_name, request.build_id, e
                );
            }
        } else {
            if let Err(e) = deploy_service(request, &build_result, config, &config_file).await {
                error!(
                    "Failed to deploy service '{}' from build {}: {}",
                    build_result.service_name, request.build_id, e
                );
            }
        }
    }

    Ok(())
}

async fn deploy_service(
    request: &DeploymentRequest,
    build_result: &build_results::Model,
    config: &DeployerConfig,
    config_file: &kennel_config::KennelConfig,
) -> Result<()> {
    let store_path = build_result
        .store_path
        .as_ref()
        .ok_or_else(|| crate::DeployerError::Other(anyhow::anyhow!("No store path")))?;

    info!(
        "Deploying service '{}' from store path: {}",
        build_result.service_name, store_path
    );

    let branch_sanitized = utils::sanitize_identifier(&request.git_ref);

    // Check for existing active deployment (blue-green)
    let existing_deployment = config
        .store
        .deployments()
        .find_active_by_ref(
            &request.project_name,
            &request.git_ref,
            &build_result.service_name,
        )
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    let unit_name = format!(
        "kennel-{}-{}-{}",
        request.project_name, branch_sanitized, build_result.service_name
    );
    let username = utils::sanitize_username(
        &request.project_name,
        &branch_sanitized,
        &build_result.service_name,
    );

    user::ensure_user_exists(&username).await?;

    let work_dir = PathBuf::from(kennel_config::constants::SERVICES_BASE_DIR)
        .join(&request.project_name)
        .join(&branch_sanitized)
        .join(&build_result.service_name);
    tokio::fs::create_dir_all(&work_dir).await?;

    let port = config
        .store
        .port_allocations()
        .find_available_port()
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))? as u16;

    // Check if service needs preview database
    let service_config = config_file.services.get(&build_result.service_name);

    let preview_db_num = if service_config.map(|s| s.preview_database).unwrap_or(false) {
        // Allocate preview database
        match config
            .store
            .preview_databases()
            .allocate(&request.project_name, &request.git_ref)
            .await
        {
            Ok(db_num) => {
                info!(
                    "Allocated preview database {} for {}/{}",
                    db_num, request.project_name, request.git_ref
                );
                Some(db_num)
            }
            Err(e) => {
                error!("Failed to allocate preview database: {}", e);
                return Err(crate::DeployerError::Other(anyhow::anyhow!(
                    "Failed to allocate preview database: {}",
                    e
                )));
            }
        }
    } else {
        None
    };

    let env_vars = vec![("PORT".to_string(), port.to_string())];

    let mut env_vars_with_db = env_vars.clone();
    if let Some(db_num) = preview_db_num {
        env_vars_with_db.push((
            "VALKEY_URL".to_string(),
            format!("redis://127.0.0.1:6379/{}", db_num),
        ));
        env_vars_with_db.push((
            "DATABASE_URL".to_string(),
            format!(
                "postgresql://127.0.0.1:5432/{}_{}",
                request.project_name.replace('-', "_"),
                branch_sanitized.replace('-', "_")
            ),
        ));
    }

    let secrets_path = secrets::generate_env_file(
        &request.project_name,
        &branch_sanitized,
        &build_result.service_name,
        &env_vars_with_db,
    )
    .await?;

    let unit_content = systemd::generate_service_unit(
        &build_result.service_name,
        store_path,
        port,
        &username,
        &work_dir,
        &[],
        Some(&secrets_path),
    );

    systemd::install_unit(&unit_name, &unit_content).await?;
    systemd::daemon_reload().await?;
    systemd::enable_unit(&unit_name).await?;
    systemd::start_unit(&unit_name).await?;

    let service_config = config_file.services.get(&build_result.service_name);
    let health_check_path = service_config
        .map(|s| s.health_check_path.as_str())
        .unwrap_or("/health");
    let health_check_timeout = service_config
        .map(|s| s.health_check_timeout_secs)
        .unwrap_or(30);

    if let Err(e) = health::check_health(port, health_check_path, health_check_timeout).await {
        error!("Health check failed for {}: {}", unit_name, e);
        systemd::stop_unit(&unit_name).await?;
        return Err(e);
    }

    let deployment = deployments::ActiveModel {
        project_name: sea_orm::ActiveValue::Set(request.project_name.clone()),
        git_ref: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        service_name: sea_orm::ActiveValue::Set(build_result.service_name.clone()),
        branch: sea_orm::ActiveValue::Set(request.git_ref.clone()),
        branch_slug: sea_orm::ActiveValue::Set(branch_sanitized.clone()),
        environment: sea_orm::ActiveValue::Set(determine_environment(&request.git_ref)),
        store_path: sea_orm::ActiveValue::Set(Some(store_path.clone())),
        port: sea_orm::ActiveValue::Set(Some(port as i32)),
        status: sea_orm::ActiveValue::Set(DeploymentStatus::Active),
        domain: sea_orm::ActiveValue::Set(utils::generate_deployment_domain(
            &build_result.service_name,
            &branch_sanitized,
            &request.project_name,
            &config.base_domain,
        )),
        ..Default::default()
    };

    let new_deployment = config
        .store
        .deployments()
        .create(deployment)
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    config
        .store
        .port_allocations()
        .allocate_for_deployment(
            port as i32,
            new_deployment.id,
            &request.project_name,
            &build_result.service_name,
            &branch_sanitized,
        )
        .await
        .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))?;

    info!(
        "Successfully deployed service '{}' on port {}",
        build_result.service_name, port
    );

    // Create DNS records for custom domain if configured
    let service_config = config_file.services.get(&build_result.service_name);
    if let Some(dns_manager) = &config.dns_manager
        && let Some(custom_domain) = service_config.and_then(|s| s.custom_domain.as_ref())
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
            port: Some(port),
            store_path: Some(store_path.clone()),
            spa: false,
        };

        if let Err(e) = router_tx.send(update) {
            warn!("Failed to send router update: {}", e);
        }
    }

    // Drain and tear down old deployment (blue-green)
    if let Some(old_deployment) = existing_deployment {
        let old_deployment_id = old_deployment.id;
        let old_port = old_deployment.port;

        info!(
            "Blue-green deployment: waiting 30s to drain connections for old deployment {}",
            old_deployment_id
        );

        tokio::time::sleep(kennel_config::constants::BLUE_GREEN_DRAIN_TIMEOUT).await;

        info!("Tearing down old deployment {}", old_deployment_id);

        // Mark for teardown
        let mut old_active = old_deployment.into_active_model();
        old_active.status = sea_orm::ActiveValue::Set(DeploymentStatus::TearingDown);
        if let Err(e) = config
            .store
            .deployments()
            .update(old_active)
            .await
            .map_err(|e| crate::DeployerError::Other(anyhow::anyhow!(e)))
        {
            error!("Failed to mark old deployment for teardown: {}", e);
        }

        // Stop old service
        let old_unit = format!(
            "kennel-{}-{}-{}",
            request.project_name, branch_sanitized, build_result.service_name
        );

        if let Err(e) = systemd::stop_unit(&old_unit).await {
            warn!("Failed to stop old unit {}: {}", old_unit, e);
        }

        // Release old port if different from new one
        if let Some(old_port_val) = old_port
            && old_port_val as u16 != port
        {
            let _ = config
                .store
                .port_allocations()
                .release_port(old_port_val)
                .await;
        }
    }

    Ok(())
}
