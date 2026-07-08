use crate::AppState;
use crate::caddy::CaddyClient;
use crate::forgejo::CommitStatus;
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
    branch: &'a str,
    branch_slug: &'a str,
    environment: &'a Environment,
}

/// Result of a deploy attempt for a branch's intent
pub enum DeployOutcome {
    Deployed,
    SkippedPreview,
}

pub async fn deploy_request(
    state: &AppState,
    request: &::entity::deploy_requests::Model,
    build: &::entity::builds::Model,
) -> anyhow::Result<DeployOutcome> {
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

    let environment = Environment::from_branch(&request.branch).unwrap_or(Environment::Dev);

    if environment == Environment::Preview && !kennel_config.preview_deployments {
        return Ok(DeployOutcome::SkippedPreview);
    }

    let branch_slug = sanitize(&request.branch);
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    let existing = state
        .store
        .deployments()
        .find_by_project_branch(&build.project_id, &request.branch)
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
        branch: &request.branch,
        branch_slug: &branch_slug,
        environment: &environment,
    };

    let deploy_result: anyhow::Result<()> = async {
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
        Ok(())
    }
    .await;

    if let Some(owner) = project.owner.as_deref() {
        let (status, description) = match &deploy_result {
            Ok(()) => ("success", "deployment healthy".to_string()),
            Err(e) => ("failure", format!("{e:#}")),
        };
        let target_url = state.config.grafana_url.as_deref().and_then(|base| {
            kennel_config.services.keys().next().map(|service| {
                let unit = service_unit_name(&project.name, &branch_slug, service);
                drilldown_unit_url(base, &format!("{unit}.service"), "now-1h", "now")
            })
        });
        let posted = state
            .forgejo
            .create_commit_status(
                owner,
                &project.name,
                &build.commit_sha,
                CommitStatus {
                    state: status,
                    description: &description,
                    context: "kennel/deploy",
                    target_url: target_url.as_deref(),
                },
            )
            .await;
        if deploy_result.is_ok() {
            posted?;
        } else if let Err(post_err) = posted {
            tracing::error!(build_id = %build.id, error = %post_err, "failed to post commit status");
        }
    }

    deploy_result?;

    // Deployments now own the store paths, so drop the build's pins
    remove_build_gc_roots(&build.id).await;

    if let Some(pr_number) = crate::forgejo::pr_number_from_branch(&request.branch)
        && let Err(e) = post_pr_deployment_comment(
            state,
            &project,
            &request.branch,
            &build.commit_sha,
            pr_number,
        )
        .await
    {
        tracing::warn!(pr = pr_number, error = %e, "failed to post PR deployment comment");
    }

    Ok(DeployOutcome::Deployed)
}

async fn post_pr_deployment_comment(
    state: &AppState,
    project: &::entity::projects::Model,
    branch: &str,
    commit_sha: &str,
    pr_number: u64,
) -> anyhow::Result<()> {
    let Some(owner) = project.owner.as_deref() else {
        anyhow::bail!("project {} has no owner recorded", project.name);
    };

    let deployments = state
        .store
        .deployments()
        .find_by_project_branch(&project.id, branch)
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
        &commit_sha[..commit_sha.len().min(7)]
    ));

    state
        .forgejo
        .upsert_pr_comment(owner, &project.name, pr_number, &body)
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
        branch,
        branch_slug,
        environment,
    } = ctx;
    let domain = generate_domain(
        project_name,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
    );

    let deployment_id = match state
        .store
        .deployments()
        .find_by_project_service_branch(&build.project_id, name, branch)
        .await?
    {
        Some(existing) => existing.id,
        None => uuid::Uuid::now_v7().to_string(),
    };

    // Pin the store path against nix-gc before anything references it
    add_gc_root(&deployment_id, store_path).await?;

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
        branch: Set(branch.to_string()),
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
        branch,
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
    let system_user = service_user(&unit_name);

    let request = ResourceRequest {
        project_name: build.project_id.clone(),
        service_name: name.to_string(),
        branch_slug: branch_slug.to_string(),
        environment: *environment,
        system_user: system_user.clone(),
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

    if let Ok(vault_endpoint) = std::env::var("VAULT_ENDPOINT")
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

    // Public URL of this deployment
    let app_url = format!(
        "https://{}",
        svc_config
            .custom_domain
            .as_deref()
            .unwrap_or(domain.as_str())
    );
    env_vars.entry("APP_URL".to_string()).or_insert(app_url);

    let deployment_id = match state
        .store
        .deployments()
        .find_by_project_service_branch(&build.project_id, name, branch)
        .await?
    {
        Some(existing) => existing.id,
        None => uuid::Uuid::now_v7().to_string(),
    };

    // Pin the store paths against nix-gc before anything references them
    add_gc_root(&deployment_id, store_path).await?;
    if let Some(ref config_store_path) = build.config_store_path {
        add_gc_root(&format!("{deployment_id}-config"), config_store_path).await?;
    }

    let systemd = crate::systemd::SystemdClient::connect().await?;
    systemd
        .start_transient_unit(
            &unit_name,
            &exec_start,
            &env_vars,
            &system_user,
            build.config_store_path.as_deref(),
        )
        .await?;

    // Wait for the service to become healthy before routing traffic
    let healthy = wait_for_healthy(port).await;
    if !healthy {
        tracing::error!(service = %name, port, "healthcheck failed, stopping unit");
        let _ = systemd.stop_unit(&unit_name).await;
        anyhow::bail!("service '{name}' failed healthcheck within startup grace period");
    }

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
        branch: Set(branch.to_string()),
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

    tracing::info!(service = %name, domain = %domain, port = port, "deployed service");
    Ok(())
}

/// Upserts the Cloudflare A record for a custom domain.
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

pub(crate) async fn add_gc_root(name: &str, store_path: &str) -> anyhow::Result<()> {
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

// Drop every gc root pinned for a build's outputs (named `{build_id}-*`)
pub async fn remove_build_gc_roots(build_id: &str) {
    let dir = PathBuf::from(kennel_config::constants::GC_ROOTS_DIR);
    let prefix = format!("{build_id}-");
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(prefix.as_str())
        {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
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

pub fn build_unit_name(project_name: &str, branch: &str) -> String {
    format!(
        "kennel-build-{}-{}",
        sanitize(project_name),
        sanitize(branch)
    )
}

pub fn service_unit_name(project_name: &str, branch_slug: &str, service: &str) -> String {
    format!(
        "kennel-{}-{}-{}",
        sanitize(project_name),
        branch_slug,
        sanitize(service)
    )
}

/// Builds a Grafana Logs Drilldown URL scoped to a single systemd unit.
pub fn drilldown_unit_url(base: &str, unit: &str, from: &str, to: &str) -> String {
    let base = base.trim_end_matches('/');
    let unit_path = urlencoding::encode(unit);
    let filter = format!("unit|=|{unit}");
    let unit_filter = urlencoding::encode(&filter);
    format!(
        "{base}/a/grafana-lokiexplore-app/explore/unit/{unit_path}/logs?patterns=%5B%5D&var-ds=loki&var-filters={unit_filter}&from={from}&to={to}"
    )
}

/// Stable login name for a unit. The unit runs under this name as a `DynamicUser`
/// and postgres maps it to a same-named role over peer auth.
pub fn service_user(unit_name: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(unit_name.as_bytes());
    format!("kennel_{}", hex::encode(&digest[..8]))
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
        let mut candidates: Vec<PathBuf> = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            // Skip makeWrapper's dot-prefixed wrapped binary and exec the wrapper
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            candidates.push(entry.path());
        }
        candidates.sort();
        if let Some(path) = candidates.into_iter().next() {
            return Ok(path.to_string_lossy().to_string());
        }
    }
    Ok(store_path.to_string())
}

async fn wait_for_healthy(port: u16) -> bool {
    use kennel_config::constants::{HEALTHCHECK_INTERVAL, HEALTHCHECK_STARTUP_GRACE};

    let deadline = tokio::time::Instant::now() + HEALTHCHECK_STARTUP_GRACE;
    let mut interval = tokio::time::interval(HEALTHCHECK_INTERVAL);

    loop {
        interval.tick().await;
        if crate::health::probe(port).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
    }
}
