use entity::sea_orm_active_enums::RepoType;
use kennel_config::constants;
use kennel_store::Store;
use kennel_supervisor::{ProcessConfig, Supervisor};
use sea_orm::ActiveValue;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: String,
    repo_url: String,
    repo_type: String,
    webhook_secret_file: String,
    default_branch: String,
}

pub async fn reconcile_projects(store: Arc<Store>) -> anyhow::Result<()> {
    let projects_json = match tokio::fs::read_to_string(constants::PROJECTS_CONFIG_PATH).await {
        Ok(content) => content,
        Err(_) => {
            info!("No projects.json found, skipping project reconciliation");
            return Ok(());
        }
    };

    let projects: Vec<ProjectConfig> = serde_json::from_str(&projects_json)?;
    info!("Reconciling {} projects from configuration", projects.len());

    for project in &projects {
        if let Err(e) = reconcile_project(&store, project).await {
            warn!("Failed to reconcile project {}: {}", project.name, e);
        }
    }

    cleanup_removed_projects(&store, &projects).await?;

    Ok(())
}

pub async fn reconcile_deployments(
    store: Arc<Store>,
    supervisor: Arc<Mutex<Supervisor>>,
) -> anyhow::Result<()> {
    info!("Running startup resource reconciliation");

    reconcile_supervisor_processes(&store, &supervisor).await?;
    reconcile_static_site_symlinks(&store).await?;

    info!("Startup reconciliation complete");
    Ok(())
}

async fn reconcile_project(store: &Store, project: &ProjectConfig) -> anyhow::Result<()> {
    let webhook_secret = tokio::fs::read_to_string(&project.webhook_secret_file)
        .await?
        .trim()
        .to_string();

    let repo_type_enum = match project.repo_type.as_str() {
        "forgejo" => RepoType::Forgejo,
        "github" => RepoType::Github,
        _ => anyhow::bail!(
            "Invalid repo_type '{}' for project {}",
            project.repo_type,
            project.name
        ),
    };

    match store.projects().find_by_name(&project.name).await? {
        Some(_existing) => {
            let project_model = entity::projects::ActiveModel {
                name: ActiveValue::Unchanged(project.name.clone()),
                repo_url: ActiveValue::Set(project.repo_url.clone()),
                repo_type: ActiveValue::Set(repo_type_enum),
                webhook_secret: ActiveValue::Set(webhook_secret),
                default_branch: ActiveValue::Set(project.default_branch.clone()),
                ..Default::default()
            };

            store.projects().update(project_model).await?;
            info!("Updated project: {}", project.name);
        }
        None => {
            let project_model = entity::projects::ActiveModel {
                name: ActiveValue::Set(project.name.clone()),
                repo_url: ActiveValue::Set(project.repo_url.clone()),
                repo_type: ActiveValue::Set(repo_type_enum),
                webhook_secret: ActiveValue::Set(webhook_secret),
                default_branch: ActiveValue::Set(project.default_branch.clone()),
                ..Default::default()
            };

            store.projects().create(project_model).await?;
            info!("Created project: {}", project.name);
        }
    }

    Ok(())
}

async fn cleanup_removed_projects(
    store: &Store,
    config_projects: &[ProjectConfig],
) -> anyhow::Result<()> {
    let config_project_names: HashSet<String> =
        config_projects.iter().map(|p| p.name.clone()).collect();

    let db_projects = store.projects().list_all().await?;

    for db_project in db_projects {
        if !config_project_names.contains(&db_project.name) {
            let deployments = store
                .deployments()
                .list_by_project(&db_project.name)
                .await?;
            if deployments.is_empty() {
                info!(
                    "Deleting project with no remaining deployments: {}",
                    db_project.name
                );
                store.projects().delete(&db_project.name).await?;
            } else {
                let ids: Vec<i32> = deployments.iter().map(|d| d.id).collect();
                info!(
                    "Marking {} deployments for teardown (project {} removed from config)",
                    ids.len(),
                    db_project.name
                );
                store.deployments().mark_tearing_down(&ids).await?;
            }
        }
    }

    Ok(())
}

/// Re-start all deployed service processes through the supervisor.
/// After a restart, the supervisor has no running processes, so we
/// reconstruct ProcessConfigs from stored deployment records.
async fn reconcile_supervisor_processes(
    store: &Store,
    supervisor: &Arc<Mutex<Supervisor>>,
) -> anyhow::Result<()> {
    info!("Reconciling supervisor processes");

    let deployed = store.find_deployed_service_deployments().await?;

    for deployment in deployed {
        let process_config: Option<ProcessConfig> = deployment
            .process_config
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        match process_config {
            Some(config) => {
                info!("Re-starting process {} from stored config", config.name);
                let mut sup = supervisor.lock().await;
                if let Err(e) = sup.start(config).await {
                    error!(
                        "Failed to restart process for deployment {}: {e}",
                        deployment.id
                    );
                }
            }
            None => {
                warn!(
                    "Deployment {} has no stored process_config, skipping",
                    deployment.id
                );
            }
        }
    }

    Ok(())
}

async fn reconcile_static_site_symlinks(store: &Store) -> anyhow::Result<()> {
    info!("Reconciling static site symlinks");

    let sites_dir = std::path::Path::new(constants::SITES_BASE_DIR);
    if !sites_dir.exists() {
        return Ok(());
    }

    let deployed_static: HashSet<String> = store
        .find_deployed_static_deployments()
        .await?
        .into_iter()
        .map(|d| format!("{}/{}/{}", d.project_name, d.branch_slug, d.service_name))
        .collect();

    let mut entries = tokio::fs::read_dir(sites_dir).await?;
    while let Some(project_entry) = entries.next_entry().await? {
        if !project_entry.file_type().await?.is_dir() {
            continue;
        }

        let project_name = project_entry.file_name().to_string_lossy().to_string();
        let project_path = project_entry.path();

        let mut branch_entries = tokio::fs::read_dir(&project_path).await?;
        while let Some(branch_entry) = branch_entries.next_entry().await? {
            if !branch_entry.file_type().await?.is_dir() {
                continue;
            }

            let branch_name = branch_entry.file_name().to_string_lossy().to_string();
            let branch_path = branch_entry.path();

            let mut site_entries = tokio::fs::read_dir(&branch_path).await?;
            while let Some(site_entry) = site_entries.next_entry().await? {
                let site_name = site_entry.file_name().to_string_lossy().to_string();
                let symlink_path = format!("{project_name}/{branch_name}/{site_name}");

                if !deployed_static.contains(&symlink_path) {
                    info!("Removing orphaned static site symlink: {symlink_path}");
                    if let Err(e) = tokio::fs::remove_file(site_entry.path()).await {
                        warn!("Failed to remove orphaned symlink {symlink_path}: {e}");
                    }
                }
            }

            if tokio::fs::read_dir(&branch_path)
                .await?
                .next_entry()
                .await?
                .is_none()
                && let Err(e) = tokio::fs::remove_dir(&branch_path).await
            {
                warn!("Failed to remove empty branch directory {branch_name}: {e}");
            }
        }

        if tokio::fs::read_dir(&project_path)
            .await?
            .next_entry()
            .await?
            .is_none()
            && let Err(e) = tokio::fs::remove_dir(&project_path).await
        {
            warn!("Failed to remove empty project directory {project_name}: {e}");
        }
    }

    Ok(())
}
