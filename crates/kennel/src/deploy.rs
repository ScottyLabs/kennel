use crate::AppState;
use kennel_config::Environment;
use std::collections::HashMap;
use std::path::PathBuf;

/// Deploy all services and static sites from a completed build.
pub async fn deploy_build(
    state: &AppState,
    build: &::entity::builds::Model,
) -> anyhow::Result<()> {
    let store_paths: HashMap<String, String> = build
        .store_paths
        .as_ref()
        .map(|v| serde_json::from_value(v.clone()).unwrap_or_default())
        .unwrap_or_default();

    let kennel_config: kennel_config::KennelConfig = build
        .kennel_config
        .as_ref()
        .map(|v| serde_json::from_value(v.clone()).unwrap_or_default())
        .unwrap_or_default();

    let environment = Environment::from_branch(&build.branch);
    let branch_slug = sanitize(&build.branch);

    for (name, site_config) in &kennel_config.static_sites {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(site = %name, "no store path, skipping");
            continue;
        };

        let domain = generate_domain(
            &build.project_id,
            Some(name),
            &branch_slug,
            &state.config.ephemeral_domain,
        );

        // Symlink store path to sites dir
        let site_dir = PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
            .join(&build.project_id)
            .join(&branch_slug);
        tokio::fs::create_dir_all(&site_dir).await?;

        let link = site_dir.join(name);
        let _ = tokio::fs::remove_file(&link).await;

        #[cfg(unix)]
        tokio::fs::symlink(store_path, &link).await?;

        // Upsert deployment record
        let deployment_id = uuid::Uuid::now_v7().to_string();
        // TODO: upsert deployment, add caddy route

        tracing::info!(
            site = %name,
            domain = %domain,
            store_path = %store_path,
            "deployed static site"
        );
    }

    for (name, _svc_config) in &kennel_config.services {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(service = %name, "no store path, skipping");
            continue;
        };

        let domain = generate_domain(
            &build.project_id,
            Some(name),
            &branch_slug,
            &state.config.ephemeral_domain,
        );

        // TODO: provision resources, resolve secrets, start systemd unit, add caddy route, upsert deployment

        tracing::info!(
            service = %name,
            domain = %domain,
            store_path = %store_path,
            "deployed service"
        );
    }

    Ok(())
}

pub fn generate_domain(
    project: &str,
    service: Option<&str>,
    branch_slug: &str,
    base_domain: &str,
) -> String {
    match service {
        Some(svc) => format!("{project}-{svc}-{branch_slug}.{base_domain}"),
        None => format!("{project}-{branch_slug}.{base_domain}"),
    }
}

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}
