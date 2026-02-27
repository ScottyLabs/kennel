use entity::sea_orm_active_enums::RepoType;
use kennel_config::constants;
use kennel_store::Store;
use sea_orm::ActiveValue;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{info, warn};

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

pub async fn reconcile_deployments(store: Arc<Store>) -> anyhow::Result<()> {
    info!("Running startup resource reconciliation");

    reconcile_systemd_units(&store).await?;
    reconcile_port_allocations(&store).await?;
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
            info!("Removing project no longer in config: {}", db_project.name);
            store.projects().delete(&db_project.name).await?;
        }
    }

    Ok(())
}

async fn reconcile_systemd_units(store: &Store) -> anyhow::Result<()> {
    info!("Reconciling systemd units");

    let output = tokio::process::Command::new("systemctl")
        .args(["list-units", "--all", "--plain", "--no-legend", "kennel-*"])
        .output()
        .await?;

    if !output.status.success() {
        warn!("Failed to list systemd units");
        return Ok(());
    }

    let units_output = String::from_utf8_lossy(&output.stdout);
    let running_units: HashSet<String> = units_output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.first().map(|s| s.to_string())
        })
        .filter(|unit| unit.starts_with("kennel-") && unit.ends_with(".service"))
        .collect();

    let active_deployments = store.deployments().list_active().await?;
    let expected_units: HashSet<String> = active_deployments
        .iter()
        .map(|d| {
            format!(
                "kennel-{}-{}-{}.service",
                d.project_name, d.branch_slug, d.service_name
            )
        })
        .collect();

    for orphaned_unit in running_units.difference(&expected_units) {
        info!("Stopping orphaned systemd unit: {}", orphaned_unit);
        if let Err(e) = tokio::process::Command::new("systemctl")
            .args(["stop", orphaned_unit])
            .output()
            .await
        {
            warn!("Failed to stop orphaned unit {}: {}", orphaned_unit, e);
        }

        if let Err(e) = tokio::process::Command::new("systemctl")
            .args(["disable", orphaned_unit])
            .output()
            .await
        {
            warn!("Failed to disable orphaned unit {}: {}", orphaned_unit, e);
        }

        let unit_file = format!("/etc/systemd/system/{}", orphaned_unit);
        if let Err(e) = tokio::fs::remove_file(&unit_file).await {
            warn!("Failed to remove orphaned unit file {}: {}", unit_file, e);
        }
    }

    if !running_units
        .difference(&expected_units)
        .collect::<Vec<_>>()
        .is_empty()
        && let Err(e) = tokio::process::Command::new("systemctl")
            .arg("daemon-reload")
            .output()
            .await
    {
        warn!("Failed to reload systemd daemon: {}", e);
    }

    Ok(())
}

async fn reconcile_port_allocations(store: &Store) -> anyhow::Result<()> {
    info!("Reconciling port allocations");

    let all_ports = store.port_allocations().list_allocated().await?;
    let active_deployments = store.deployments().list_active().await?;
    let active_deployment_ids: HashSet<i32> = active_deployments.iter().map(|d| d.id).collect();

    for port_allocation in all_ports {
        if let Some(deployment_id) = port_allocation.deployment_id
            && !active_deployment_ids.contains(&deployment_id)
        {
            info!(
                "Releasing stale port {} allocated to non-existent deployment {}",
                port_allocation.port, deployment_id
            );
            if let Err(e) = store
                .port_allocations()
                .release_port(port_allocation.port)
                .await
            {
                warn!(
                    "Failed to release stale port {}: {}",
                    port_allocation.port, e
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

    let active_static_deployments: HashSet<String> = store
        .deployments()
        .list_active()
        .await?
        .into_iter()
        .filter(|d| d.port.is_none())
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
                let symlink_path = format!("{}/{}/{}", project_name, branch_name, site_name);

                if !active_static_deployments.contains(&symlink_path) {
                    info!("Removing orphaned static site symlink: {}", symlink_path);
                    if let Err(e) = tokio::fs::remove_file(site_entry.path()).await {
                        warn!("Failed to remove orphaned symlink {}: {}", symlink_path, e);
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
                warn!(
                    "Failed to remove empty branch directory {}: {}",
                    branch_name, e
                );
            }
        }

        if tokio::fs::read_dir(&project_path)
            .await?
            .next_entry()
            .await?
            .is_none()
            && let Err(e) = tokio::fs::remove_dir(&project_path).await
        {
            warn!(
                "Failed to remove empty project directory {}: {}",
                project_name, e
            );
        }
    }

    Ok(())
}
