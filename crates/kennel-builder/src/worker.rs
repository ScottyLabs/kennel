use crate::error::Result;
use crate::{BuilderConfig, cachix, git, nix};
use entity::build_results;
use entity::sea_orm_active_enums::{BuildResultStatus, BuildStatus};
use kennel_config::parse_kennel_toml;
use kennel_store::Store;
use sea_orm::{ActiveValue::Set, IntoActiveModel};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

pub async fn process_build(build_id: i32, config: Arc<BuilderConfig>) -> Result<()> {
    info!("Processing build {}", build_id);

    let build = update_build_status_to_building(&config.store, build_id).await?;

    if check_cancelled(&config.store, build_id).await? {
        info!("Build {} cancelled before starting", build_id);
        return Ok(());
    }

    let (project_name, git_ref, work_dir) =
        setup_build_environment(&config, &build, build_id).await?;

    let kennel_config = clone_and_parse_config(&config, &build, build_id, &work_dir).await?;

    if check_cancelled(&config.store, build_id).await? {
        info!("Build {} cancelled after clone", build_id);
        return Ok(());
    }

    let mut store_paths = Vec::new();
    let all_services_succeeded = build_all_packages(
        &config,
        &build,
        &kennel_config,
        &work_dir,
        build_id,
        &mut store_paths,
    )
    .await;

    if let Some(cachix_config) = &kennel_config.cachix
        && !store_paths.is_empty()
        && let Err(e) = cachix::push_to_cachix(cachix_config, &store_paths).await
    {
        warn!("Failed to push to Cachix: {}", e);
    }

    finalize_build(
        &config,
        build,
        build_id,
        all_services_succeeded,
        project_name,
        git_ref,
    )
    .await
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

async fn update_build_status_to_building(
    store: &Store,
    build_id: i32,
) -> Result<entity::builds::Model> {
    let build = store
        .builds()
        .find_by_id(build_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Build {} not found", build_id))?;

    let mut build_active = build.clone().into_active_model();
    build_active.status = Set(BuildStatus::Building);
    build_active.started_at = Set(Some(chrono::Utc::now().naive_utc()));
    store
        .builds()
        .update(build_active)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(build)
}

async fn setup_build_environment(
    config: &Arc<BuilderConfig>,
    build: &entity::builds::Model,
    build_id: i32,
) -> Result<(String, String, PathBuf)> {
    let project = config
        .store
        .projects()
        .find_by_name(&build.project_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project {} not found", build.project_name))?;

    let project_name = project.name.clone();
    let git_ref = build.git_ref.clone();
    let work_dir = PathBuf::from(&config.work_dir).join(build_id.to_string());

    Ok((project_name, git_ref, work_dir))
}

async fn clone_and_parse_config(
    config: &Arc<BuilderConfig>,
    build: &entity::builds::Model,
    build_id: i32,
    work_dir: &Path,
) -> Result<kennel_config::KennelConfig> {
    let project = config
        .store
        .projects()
        .find_by_name(&build.project_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project {} not found", build.project_name))?;

    info!("Cloning repository for build {}", build_id);
    if let Err(e) = git::clone(&project.repo_url, &build.commit_sha, work_dir).await {
        error!("Git clone failed for build {}: {}", build_id, e);
        mark_build_failed(&config.store, build_id, &e.to_string()).await?;
        return Err(e);
    }

    let kennel_config = parse_kennel_toml(work_dir).await.map_err(|e| {
        crate::BuilderError::Other(anyhow::anyhow!("Failed to parse kennel.toml: {}", e))
    })?;

    if kennel_config.services.is_empty() && kennel_config.static_sites.is_empty() {
        warn!(
            "No services or static sites defined in kennel.toml for build {}",
            build_id
        );
        mark_build_failed(
            &config.store,
            build_id,
            "No services or static sites defined",
        )
        .await?;
        return Err(crate::BuilderError::Other(anyhow::anyhow!(
            "No services or static sites defined"
        )));
    }

    Ok(kennel_config)
}

async fn build_all_packages(
    config: &Arc<BuilderConfig>,
    build: &entity::builds::Model,
    kennel_config: &kennel_config::KennelConfig,
    work_dir: &Path,
    build_id: i32,
    store_paths: &mut Vec<String>,
) -> bool {
    let mut all_succeeded = true;

    for service_name in kennel_config.services.keys() {
        if check_cancelled(&config.store, build_id)
            .await
            .unwrap_or(false)
        {
            info!("Build {} cancelled during service builds", build_id);
            return false;
        }

        if !build_package(
            config,
            build,
            work_dir,
            service_name,
            build_id,
            store_paths,
            true,
        )
        .await
        {
            all_succeeded = false;
        }
    }

    for site_name in kennel_config.static_sites.keys() {
        if check_cancelled(&config.store, build_id)
            .await
            .unwrap_or(false)
        {
            info!("Build {} cancelled during static site builds", build_id);
            return false;
        }

        if !build_package(
            config,
            build,
            work_dir,
            site_name,
            build_id,
            store_paths,
            false,
        )
        .await
        {
            all_succeeded = false;
        }
    }

    all_succeeded
}

async fn build_package(
    config: &Arc<BuilderConfig>,
    build: &entity::builds::Model,
    work_dir: &Path,
    package_name: &str,
    build_id: i32,
    store_paths: &mut Vec<String>,
    is_service: bool,
) -> bool {
    let package_type = if is_service { "service" } else { "static site" };

    if let Err(e) = nix::validate_service_name(package_name) {
        error!(
            "Invalid {} name '{}' for build {}: {}",
            package_type, package_name, build_id, e
        );
        record_failed_build_result(&config.store, build_id, package_name, &e.to_string()).await;
        return false;
    }

    info!(
        "Building {} '{}' for build {}",
        package_type, package_name, build_id
    );

    let recent_results = if is_service {
        config
            .store
            .build_results()
            .find_recent_successful(&build.project_name, &build.git_ref, package_name, 5)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    match nix::build(work_dir, package_name, build_id).await {
        Ok(store_path) => {
            let is_unchanged = recent_results
                .iter()
                .any(|r| r.store_path.as_ref() == Some(&store_path));

            if is_unchanged {
                info!(
                    "{} '{}' for build {} is unchanged (store path: {})",
                    package_type, package_name, build_id, store_path
                );
            } else {
                info!(
                    "Successfully built {} '{}' for build {}: {}",
                    package_type, package_name, build_id, store_path
                );
            }

            store_paths.push(store_path.clone());

            let build_result = build_results::ActiveModel {
                build_id: Set(build_id),
                service_name: Set(package_name.to_string()),
                status: Set(BuildResultStatus::Success),
                store_path: Set(Some(store_path)),
                log_path: Set(Some(format!(
                    "{}/{}/{}.log",
                    kennel_config::constants::LOGS_DIR,
                    build_id,
                    package_name
                ))),
                ..Default::default()
            };

            if let Err(e) = config.store.build_results().create(build_result).await {
                error!("Failed to record build result: {}", e);
            }

            true
        }
        Err(e) => {
            error!(
                "Nix build failed for {} '{}' in build {}: {}",
                package_type, package_name, build_id, e
            );
            record_failed_build_result(&config.store, build_id, package_name, &e.to_string()).await;
            false
        }
    }
}

async fn finalize_build(
    config: &Arc<BuilderConfig>,
    build: entity::builds::Model,
    build_id: i32,
    all_succeeded: bool,
    project_name: String,
    git_ref: String,
) -> Result<()> {
    let mut build_active = build.into_active_model();

    if all_succeeded {
        build_active.status = Set(BuildStatus::Success);
        build_active.finished_at = Set(Some(chrono::Utc::now().naive_utc()));
        config
            .store
            .builds()
            .update(build_active)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

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
