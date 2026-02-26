use crate::error::Result;
use crate::{BuilderConfig, cachix, git, nix};
use entity::build_results;
use entity::sea_orm_active_enums::{BuildResultStatus, BuildStatus};
use kennel_config::parse_kennel_toml;
use kennel_store::Store;
use sea_orm::{ActiveValue::Set, IntoActiveModel};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

pub async fn process_build(build_id: i64, config: Arc<BuilderConfig>) -> Result<()> {
    info!("Processing build {}", build_id);

    // Convert i64 to i32 for database query
    let build_id_i32 = build_id as i32;

    // Update build status to 'building'
    let build = config
        .store
        .builds()
        .find_by_id(build_id_i32)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Build {} not found", build_id))?;

    let mut build_active = build.clone().into_active_model();
    build_active.status = Set(BuildStatus::Building);
    build_active.started_at = Set(Some(chrono::Utc::now().naive_utc()));
    config
        .store
        .builds()
        .update(build_active)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    // Check for cancellation
    if check_cancelled(&config.store, build_id_i32).await? {
        info!("Build {} cancelled before starting", build_id);
        return Ok(());
    }

    // Fetch project details
    let project = config
        .store
        .projects()
        .find_by_name(&build.project_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project {} not found", build.project_name))?;

    // Clone values needed after move
    let project_name = project.name.clone();
    let git_ref = build.git_ref.clone();

    // Create work directory
    let work_dir = PathBuf::from(&config.work_dir).join(build_id.to_string());

    // Clone repository
    info!("Cloning repository for build {}", build_id);
    if let Err(e) = git::clone(&project.repo_url, &build.commit_sha, &work_dir).await {
        error!("Git clone failed for build {}: {}", build_id, e);
        mark_build_failed(&config.store, build_id_i32, &e.to_string()).await?;
        return Err(e);
    }

    // Check for cancellation after clone
    if check_cancelled(&config.store, build_id_i32).await? {
        info!("Build {} cancelled after clone", build_id);
        return Ok(());
    }

    // Parse kennel.toml
    let kennel_config = parse_kennel_toml(&work_dir).await.map_err(|e| {
        crate::BuilderError::Other(anyhow::anyhow!("Failed to parse kennel.toml: {}", e))
    })?;

    if kennel_config.services.is_empty() && kennel_config.static_sites.is_empty() {
        warn!(
            "No services or static sites defined in kennel.toml for build {}",
            build_id
        );
        mark_build_failed(
            &config.store,
            build_id_i32,
            "No services or static sites defined",
        )
        .await?;
        return Err(crate::BuilderError::Other(anyhow::anyhow!(
            "No services or static sites defined"
        )));
    }

    let mut store_paths = Vec::new();
    let mut all_services_succeeded = true;

    // Build all services
    for service_name in kennel_config.services.keys() {
        if check_cancelled(&config.store, build_id_i32).await? {
            info!("Build {} cancelled during service builds", build_id);
            return Ok(());
        }

        // Validate service name before building
        if let Err(e) = nix::validate_service_name(service_name) {
            error!(
                "Invalid service name '{}' for build {}: {}",
                service_name, build_id, e
            );
            record_failed_build_result(&config.store, build_id_i32, service_name, &e.to_string())
                .await;
            all_services_succeeded = false;
            continue;
        }

        info!("Building service '{}' for build {}", service_name, build_id);

        // Check for unchanged build by comparing against recent builds
        let recent_results = config
            .store
            .build_results()
            .find_recent_successful(&build.project_name, &build.git_ref, service_name, 5)
            .await
            .unwrap_or_default();

        match nix::build(&work_dir, service_name, build_id).await {
            Ok(store_path) => {
                // Check if this build is unchanged, i.e. store path matches recent build
                let is_unchanged = recent_results
                    .iter()
                    .any(|r| r.store_path.as_ref() == Some(&store_path));

                if is_unchanged {
                    info!(
                        "Service '{}' for build {} is unchanged (store path: {})",
                        service_name, build_id, store_path
                    );
                } else {
                    info!(
                        "Successfully built service '{}' for build {}: {}",
                        service_name, build_id, store_path
                    );
                }

                store_paths.push(store_path.clone());

                // Record successful build result
                let build_result = build_results::ActiveModel {
                    build_id: Set(build_id_i32),
                    service_name: Set(service_name.to_string()),
                    status: Set(BuildResultStatus::Success),
                    store_path: Set(Some(store_path)),
                    log_path: Set(Some(format!(
                        "{}/{}/{}.log",
                        kennel_config::constants::LOGS_DIR,
                        build_id,
                        service_name
                    ))),
                    ..Default::default()
                };

                if let Err(e) = config.store.build_results().create(build_result).await {
                    error!("Failed to record build result: {}", e);
                }
            }
            Err(e) => {
                error!(
                    "Nix build failed for service '{}' in build {}: {}",
                    service_name, build_id, e
                );
                all_services_succeeded = false;
                record_failed_build_result(
                    &config.store,
                    build_id_i32,
                    service_name,
                    &e.to_string(),
                )
                .await;
            }
        }
    }

    // Build all static sites
    for site_name in kennel_config.static_sites.keys() {
        if check_cancelled(&config.store, build_id_i32).await? {
            info!("Build {} cancelled during static site builds", build_id);
            return Ok(());
        }

        // Validate site name before building
        if let Err(e) = nix::validate_service_name(site_name) {
            error!(
                "Invalid site name '{}' for build {}: {}",
                site_name, build_id, e
            );
            record_failed_build_result(&config.store, build_id_i32, site_name, &e.to_string())
                .await;
            all_services_succeeded = false;
            continue;
        }

        info!(
            "Building static site '{}' for build {}",
            site_name, build_id
        );

        match nix::build(&work_dir, site_name, build_id).await {
            Ok(store_path) => {
                info!(
                    "Successfully built static site '{}' for build {}: {}",
                    site_name, build_id, store_path
                );

                store_paths.push(store_path.clone());

                // Record successful build result
                let build_result = build_results::ActiveModel {
                    build_id: Set(build_id_i32),
                    service_name: Set(site_name.to_string()),
                    status: Set(BuildResultStatus::Success),
                    store_path: Set(Some(store_path)),
                    log_path: Set(Some(format!(
                        "{}/{}/{}.log",
                        kennel_config::constants::LOGS_DIR,
                        build_id,
                        site_name
                    ))),
                    ..Default::default()
                };

                if let Err(e) = config.store.build_results().create(build_result).await {
                    error!("Failed to record build result: {}", e);
                }
            }
            Err(e) => {
                error!(
                    "Nix build failed for static site '{}' in build {}: {}",
                    site_name, build_id, e
                );
                all_services_succeeded = false;
                record_failed_build_result(&config.store, build_id_i32, site_name, &e.to_string())
                    .await;
            }
        }
    }

    // Push to Cachix if configured
    if let Some(cachix_config) = &kennel_config.cachix
        && !store_paths.is_empty()
        && let Err(e) = cachix::push_to_cachix(cachix_config, &store_paths).await
    {
        warn!("Failed to push to Cachix: {}", e);
    }

    // Mark build as success or failed
    let mut build_active = build.into_active_model();
    if all_services_succeeded {
        build_active.status = Set(BuildStatus::Success);
        build_active.finished_at = Set(Some(chrono::Utc::now().naive_utc()));
        config
            .store
            .builds()
            .update(build_active)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // Send deployment request
        if let Err(e) = config
            .deploy_tx
            .send(crate::DeploymentRequest {
                build_id,
                project_name,
                git_ref,
            })
            .await
        {
            error!("Failed to send deployment request: {}", e);
        }

        Ok(())
    } else {
        build_active.status = Set(BuildStatus::Failed);
        build_active.finished_at = Set(Some(chrono::Utc::now().naive_utc()));
        config
            .store
            .builds()
            .update(build_active)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Err(crate::BuilderError::Other(anyhow::anyhow!(
            "One or more builds failed"
        )))
    }
}

async fn record_failed_build_result(
    store: &Store,
    build_id: i32,
    service_name: &str,
    error_message: &str,
) {
    let build_result = build_results::ActiveModel {
        build_id: Set(build_id),
        service_name: Set(service_name.to_string()),
        status: Set(BuildResultStatus::Failed),
        store_path: Set(None),
        log_path: Set(Some(format!(
            "{}/{}/{}.log",
            kennel_config::constants::LOGS_DIR,
            build_id,
            service_name
        ))),
        error_message: Set(Some(error_message.to_string())),
        ..Default::default()
    };

    if let Err(e) = store.build_results().create(build_result).await {
        error!("Failed to record build result: {}", e);
    }
}

async fn check_cancelled(store: &Store, build_id: i32) -> Result<bool> {
    let build = store.builds().find_by_id(build_id).await?;

    Ok(build
        .map(|b| b.status == BuildStatus::Cancelled)
        .unwrap_or(false))
}

async fn mark_build_failed(store: &Store, build_id: i32, error: &str) -> Result<()> {
    let build = store
        .builds()
        .find_by_id(build_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Build {} not found", build_id))?;

    let mut build_active = build.into_active_model();
    build_active.status = Set(BuildStatus::Failed);
    build_active.finished_at = Set(Some(chrono::Utc::now().naive_utc()));

    store
        .builds()
        .update(build_active)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    warn!("Build {} marked as failed: {}", build_id, error);
    Ok(())
}
