use crate::AppState;
use crate::caddy::CaddyClient;
use kennel_config::Environment;
use kennel_provision::{ResourceProvider, ResourceRequest};
use sea_orm::ActiveValue::Set;
use std::collections::HashMap;
use std::path::PathBuf;

struct DeployCtx<'a> {
    state: &'a AppState,
    caddy: &'a CaddyClient,
    project_name: &'a str,
    build: &'a ::entity::builds::Model,
    branch_slug: &'a str,
    environment: &'a Environment,
}

pub async fn deploy_build(state: &AppState, build: &::entity::builds::Model) -> anyhow::Result<()> {
    let project = state
        .store
        .projects()
        .find_by_id(&build.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {} not found", build.project_id))?;

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

    let existing = state
        .store
        .deployments()
        .find_by_project_branch(&build.project_id, &build.branch)
        .await?;
    let systemd = crate::systemd::SystemdClient::connect().await?;
    for deployment in &existing {
        let still_present = match deployment.service_type.as_str() {
            "static" => kennel_config
                .static_sites
                .contains_key(&deployment.service_name),
            _ => kennel_config
                .services
                .contains_key(&deployment.service_name),
        };
        if still_present {
            continue;
        }
        if let Err(e) =
            crate::teardown::teardown_deployment(state, deployment, &systemd, &caddy).await
        {
            tracing::warn!(
                service = %deployment.service_name,
                error = %e,
                "orphan teardown failed",
            );
        } else {
            tracing::info!(
                service = %deployment.service_name,
                "torn down orphan deployment removed from kennel config",
            );
        }
    }

    let ctx = DeployCtx {
        state,
        caddy: &caddy,
        project_name: &project.name,
        build,
        branch_slug: &branch_slug,
        environment: &environment,
    };

    for (name, site_config) in &kennel_config.static_sites {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(site = %name, "no store path, skipping");
            continue;
        };

        deploy_static_site(&ctx, name, store_path, site_config).await?;
    }

    for (name, svc_config) in &kennel_config.services {
        let Some(store_path) = store_paths.get(name) else {
            tracing::warn!(service = %name, "no store path, skipping");
            continue;
        };

        deploy_service(&ctx, name, store_path, svc_config).await?;
    }

    if let Some(pr_number) = crate::forgejo::pr_number_from_branch(&build.branch)
        && let Err(e) = post_pr_deployment_comment(state, &project, build, pr_number).await
    {
        tracing::warn!(pr = pr_number, error = %e, "failed to post PR deployment comment");
    }

    Ok(())
}

async fn post_pr_deployment_comment(
    state: &AppState,
    project: &::entity::projects::Model,
    build: &::entity::builds::Model,
    pr_number: u64,
) -> anyhow::Result<()> {
    let Some((owner, repo)) = crate::forgejo::parse_owner_repo(&project.repo_url) else {
        anyhow::bail!("could not parse owner/repo from {}", project.repo_url);
    };

    let deployments = state
        .store
        .deployments()
        .find_by_project_branch(&project.id, &build.branch)
        .await?;

    if deployments.is_empty() {
        return Ok(());
    }

    let mut body = String::from("### Kennel Deployments\n\n| Service | URL |\n| --- | --- |\n");
    for d in &deployments {
        body.push_str(&format!("| {} | https://{} |\n", d.service_name, d.domain));
        if let Some(ref custom) = d.custom_domain {
            body.push_str(&format!(
                "| {} (custom) | https://{} |\n",
                d.service_name, custom
            ));
        }
    }
    body.push_str(&format!(
        "\nLast updated for commit `{}`.\n",
        &build.commit_sha[..build.commit_sha.len().min(7)]
    ));

    state
        .forgejo
        .upsert_pr_comment(&owner, &repo, pr_number, &body)
        .await
}

async fn deploy_static_site(
    ctx: &DeployCtx<'_>,
    name: &str,
    store_path: &str,
    site_config: &kennel_config::StaticSiteConfig,
) -> anyhow::Result<()> {
    let &DeployCtx {
        state,
        caddy,
        project_name,
        build,
        branch_slug,
        environment,
    } = ctx;
    let domain = generate_domain(
        project_name,
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

    let deployment_id = match state
        .store
        .deployments()
        .find_by_project_service_branch(&build.project_id, name, &build.branch)
        .await?
    {
        Some(existing) => existing.id,
        None => uuid::Uuid::now_v7().to_string(),
    };
    let route_id = format!("kennel-{deployment_id}");

    caddy
        .add_static_route(&route_id, &domain, store_path, site_config.spa)
        .await?;

    if let Some(ref custom_domain) = site_config.custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_static_route(&custom_route_id, custom_domain, store_path, site_config.spa)
            .await?;
        ensure_dns_record(state, project_name, custom_domain).await;
    }

    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id.clone()),
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

    add_gc_root(&deployment_id, store_path).await?;

    tracing::info!(site = %name, domain = %domain, "deployed static site");
    Ok(())
}

async fn deploy_service(
    ctx: &DeployCtx<'_>,
    name: &str,
    store_path: &str,
    svc_config: &kennel_config::ServiceConfig,
) -> anyhow::Result<()> {
    let &DeployCtx {
        state,
        caddy,
        project_name,
        build,
        branch_slug,
        environment,
    } = ctx;
    let domain = generate_domain(
        project_name,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
    );
    let unit_name = format!(
        "kennel-{}-{}-{}",
        sanitize(project_name),
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

    if let Ok(vault_endpoint) = dotenvy::var("VAULT_ENDPOINT")
        && let Some(ref config_store_path) = build.config_store_path
    {
        let env_str = environment.to_string();
        match crate::secrets::resolve(
            std::path::Path::new(config_store_path),
            &env_str,
            &vault_endpoint,
        ) {
            Ok(secrets) => env_vars.extend(secrets),
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "secret resolution failed for service '{name}': {e}"
                ));
            }
        }
    }

    let exec_start = find_executable(store_path).await?;
    let port = allocate_port(&unit_name);
    env_vars.insert("PORT".to_string(), port.to_string());
    env_vars.insert("COMMIT_HASH".to_string(), build.commit_sha.clone());

    let systemd = crate::systemd::SystemdClient::connect().await?;
    systemd
        .start_transient_unit(&unit_name, &exec_start, &env_vars)
        .await?;

    let deployment_id = match state
        .store
        .deployments()
        .find_by_project_service_branch(&build.project_id, name, &build.branch)
        .await?
    {
        Some(existing) => existing.id,
        None => uuid::Uuid::now_v7().to_string(),
    };
    let route_id = format!("kennel-{deployment_id}");

    caddy.add_proxy_route(&route_id, &domain, port).await?;

    if let Some(ref custom_domain) = svc_config.custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_proxy_route(&custom_route_id, custom_domain, port)
            .await?;
        ensure_dns_record(state, project_name, custom_domain).await;
    }

    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id.clone()),
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
        config_store_path: Set(build.config_store_path.clone()),
        ..Default::default()
    };

    state.store.deployments().upsert(model).await?;

    add_gc_root(&deployment_id, store_path).await?;
    if let Some(ref config_store_path) = build.config_store_path {
        add_gc_root(&format!("{deployment_id}-config"), config_store_path).await?;
    }

    tracing::info!(service = %name, domain = %domain, port = port, "deployed service");
    Ok(())
}

/// Best-effort upsert of a Cloudflare A record for a custom domain. Failures
/// are logged but never fail the deploy; DNS automation is opportunistic when
/// a matching zone is configured, and external records (manual, tofu-managed,
/// etc.) remain valid.
pub async fn ensure_dns_record(state: &AppState, project_name: &str, fqdn: &str) {
    let Some(cf) = &state.cloudflare else {
        return;
    };
    match cf.upsert_a_record(project_name, fqdn).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!(fqdn = %fqdn, "no cloudflare zone configured for domain");
        }
        Err(e) => {
            tracing::warn!(fqdn = %fqdn, error = %e, "failed to upsert cloudflare A record");
        }
    }
}

async fn add_gc_root(name: &str, store_path: &str) -> anyhow::Result<()> {
    let gc_root = PathBuf::from(kennel_config::constants::GC_ROOTS_DIR).join(name);
    let output = tokio::process::Command::new("nix-store")
        .args(["--realise", store_path, "--add-root"])
        .arg(&gc_root)
        .output()
        .await?;

    anyhow::ensure!(
        output.status.success(),
        "nix-store --add-root failed for {}: {}",
        gc_root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

pub async fn remove_gc_roots(deployment_id: &str) {
    let dir = PathBuf::from(kennel_config::constants::GC_ROOTS_DIR);
    let _ = tokio::fs::remove_file(dir.join(deployment_id)).await;
    let _ = tokio::fs::remove_file(dir.join(format!("{deployment_id}-config"))).await;
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

pub async fn find_executable(store_path: &str) -> anyhow::Result<String> {
    let bin_dir = PathBuf::from(store_path).join("bin");
    if bin_dir.exists() {
        let mut entries = tokio::fs::read_dir(&bin_dir).await?;
        if let Some(entry) = entries.next_entry().await? {
            return Ok(entry.path().to_string_lossy().to_string());
        }
    }
    Ok(store_path.to_string())
}
