use crate::caddy::CaddyClient;
use crate::AppState;
use kennel_config::Environment;
use kennel_provision::{ResourceProvider, ResourceRequest};
use sea_orm::ActiveValue::Set;
use std::collections::HashMap;
use std::path::PathBuf;

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
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    for (name, site_config) in &kennel_config.static_sites {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(site = %name, "no store path, skipping");
            continue;
        };

        deploy_static_site(
            state,
            &caddy,
            build,
            name,
            store_path,
            &branch_slug,
            &environment,
            site_config,
        )
        .await?;
    }

    for (name, svc_config) in &kennel_config.services {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(service = %name, "no store path, skipping");
            continue;
        };

        deploy_service(
            state,
            &caddy,
            build,
            name,
            store_path,
            &branch_slug,
            &environment,
            svc_config,
        )
        .await?;
    }

    Ok(())
}

async fn deploy_static_site(
    state: &AppState,
    caddy: &CaddyClient,
    build: &::entity::builds::Model,
    name: &str,
    store_path: &str,
    branch_slug: &str,
    environment: &Environment,
    site_config: &kennel_config::StaticSiteConfig,
) -> anyhow::Result<()> {
    let domain = generate_domain(
        &build.project_id,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
    );

    let site_dir = PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
        .join(&build.project_id)
        .join(branch_slug);
    tokio::fs::create_dir_all(&site_dir).await?;

    let link = site_dir.join(name);
    let temp = site_dir.join(format!("{name}.new"));
    let _ = tokio::fs::remove_file(&temp).await;

    #[cfg(unix)]
    tokio::fs::symlink(store_path, &temp).await?;

    if link.exists() {
        tokio::fs::remove_file(&link).await?;
    }
    tokio::fs::rename(&temp, &link).await?;

    let deployment_id = uuid::Uuid::now_v7().to_string();
    let route_id = format!("kennel-{deployment_id}");

    caddy
        .add_static_route(&route_id, &domain, store_path, site_config.spa)
        .await?;

    if let Some(ref custom_domain) = site_config.custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_static_route(&custom_route_id, custom_domain, store_path, site_config.spa)
            .await?;
    }

    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id),
        project_id: Set(build.project_id.clone()),
        service_name: Set(name.to_string()),
        service_type: Set("static".to_string()),
        branch: Set(build.branch.clone()),
        branch_slug: Set(branch_slug.to_string()),
        environment: Set(environment.to_string()),
        commit_sha: Set(build.commit_sha.clone()),
        store_path: Set(store_path.to_string()),
        domain: Set(domain.clone()),
        custom_domain: Set(site_config.custom_domain.clone()),
        spa: Set(site_config.spa),
        ..Default::default()
    };

    state.store.deployments().upsert(model).await?;

    tracing::info!(site = %name, domain = %domain, "deployed static site");
    Ok(())
}

async fn deploy_service(
    state: &AppState,
    caddy: &CaddyClient,
    build: &::entity::builds::Model,
    name: &str,
    store_path: &str,
    branch_slug: &str,
    environment: &Environment,
    svc_config: &kennel_config::ServiceConfig,
) -> anyhow::Result<()> {
    let domain = generate_domain(
        &build.project_id,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
    );
    let unit_name = format!(
        "kennel-{}-{}-{}",
        sanitize(&build.project_id),
        branch_slug,
        sanitize(name)
    );

    let request = ResourceRequest {
        project_name: build.project_id.clone(),
        service_name: name.to_string(),
        branch_slug: branch_slug.to_string(),
        environment: *environment,
    };

    let mut env_vars: HashMap<String, String> = HashMap::new();
    for provider in &state.providers {
        match provider.provision(&request).await {
            Ok(vars) => env_vars.extend(vars),
            Err(e) => {
                tracing::warn!(provider = provider.name(), error = %e, "resource provision failed");
            }
        }
    }

    // TODO: resolve secrets via secretspec/OpenBao

    let exec_start = find_executable(store_path).await?;
    let port = allocate_port(&unit_name);
    env_vars.insert("PORT".to_string(), port.to_string());

    let systemd = crate::systemd::SystemdClient::connect().await?;
    systemd
        .start_transient_unit(&unit_name, &exec_start, Some(&unit_name), &env_vars)
        .await?;

    let deployment_id = uuid::Uuid::now_v7().to_string();
    let route_id = format!("kennel-{deployment_id}");

    caddy
        .add_proxy_route(&route_id, &domain, port)
        .await?;

    if let Some(ref custom_domain) = svc_config.custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_proxy_route(&custom_route_id, custom_domain, port)
            .await?;
    }

    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id),
        project_id: Set(build.project_id.clone()),
        service_name: Set(name.to_string()),
        service_type: Set("service".to_string()),
        branch: Set(build.branch.clone()),
        branch_slug: Set(branch_slug.to_string()),
        environment: Set(environment.to_string()),
        commit_sha: Set(build.commit_sha.clone()),
        store_path: Set(store_path.to_string()),
        domain: Set(domain.clone()),
        custom_domain: Set(svc_config.custom_domain.clone()),
        spa: Set(false),
        unit_name: Set(Some(unit_name)),
        port: Set(Some(port as i32)),
        ..Default::default()
    };

    state.store.deployments().upsert(model).await?;

    tracing::info!(service = %name, domain = %domain, port = port, "deployed service");
    Ok(())
}

fn generate_domain(project: &str, service: &str, branch_slug: &str, base_domain: &str) -> String {
    format!("{project}-{service}-{branch_slug}.{base_domain}")
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

fn allocate_port(unit_name: &str) -> u16 {
    use kennel_config::constants::{PORT_RANGE_SIZE, PORT_RANGE_START};
    let hash = unit_name
        .as_bytes()
        .iter()
        .fold(0u32, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u32));
    PORT_RANGE_START + (hash % PORT_RANGE_SIZE as u32) as u16
}

async fn find_executable(store_path: &str) -> anyhow::Result<String> {
    let bin_dir = PathBuf::from(store_path).join("bin");
    if bin_dir.exists() {
        let mut entries = tokio::fs::read_dir(&bin_dir).await?;
        if let Some(entry) = entries.next_entry().await? {
            return Ok(entry.path().to_string_lossy().to_string());
        }
    }
    Ok(store_path.to_string())
}
