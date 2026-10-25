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
    project: &'a ::entity::projects::Model,
    commit_sha: &'a str,
    config_store_path: &'a str,
    branch: &'a str,
    branch_slug: &'a str,
    environment: &'a Environment,
    kennel_config: &'a kennel_config::KennelConfig,
}

/// Result of a deploy attempt for a branch's intent
pub enum DeployOutcome {
    Deployed,
    SkippedPreview,
}

/// Step of a deploy that failed, exported as the `stage` label of `kennel_deploy_failures`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureStage {
    /// Substituting the commit's artifact from the binary cache
    Fetch,
    /// The `kennel.json` schema version does not match this kennel
    Config,
    /// Creating the postgres, valkey, or garage resources the project declares
    Provision,
    /// Resolving the project's secrets from OpenBao
    Secrets,
    /// Starting the service's transient systemd unit
    Systemd,
    /// The service did not pass its health check within the startup grace period
    Healthcheck,
    /// Adding the deployment's caddy routes
    Routing,
    /// Kennel's own bookkeeping such as its database, gc roots, and site links
    Internal,
}

impl FailureStage {
    pub const ALL: [Self; 8] = [
        Self::Fetch,
        Self::Config,
        Self::Provision,
        Self::Secrets,
        Self::Systemd,
        Self::Healthcheck,
        Self::Routing,
        Self::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Config => "config",
            Self::Provision => "provision",
            Self::Secrets => "secrets",
            Self::Systemd => "systemd",
            Self::Healthcheck => "healthcheck",
            Self::Routing => "routing",
            Self::Internal => "internal",
        }
    }
}

/// Deploy error tagged with the stage that produced it
#[derive(Debug)]
pub struct StageError {
    pub stage: FailureStage,
    error: anyhow::Error,
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.error)
    }
}

impl std::error::Error for StageError {}

trait AtStage<T> {
    fn at(self, stage: FailureStage) -> Result<T, StageError>;
}

impl<T, E: Into<anyhow::Error>> AtStage<T> for Result<T, E> {
    fn at(self, stage: FailureStage) -> Result<T, StageError> {
        self.map_err(|e| StageError {
            stage,
            error: e.into(),
        })
    }
}

/// Provision the resources the config declares, gated by schema version and the `resources` list
pub async fn provision_declared(
    state: &AppState,
    config: &kennel_config::KennelConfig,
    request: &ResourceRequest,
) -> Result<HashMap<String, String>, StageError> {
    if !config.is_compatible() {
        return Err(StageError {
            stage: FailureStage::Config,
            error: anyhow::anyhow!(
                "kennel.json schema version {} does not match expected {}. Update the project's kennel input and push again.",
                config.version,
                kennel_config::constants::KENNEL_CONFIG_SCHEMA_VERSION
            ),
        });
    }

    let mut env_vars = HashMap::new();
    for provider in &state.providers {
        if !config.provides(provider.name()) {
            continue;
        }

        let vars = provider
            .provision(request, config)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "provisioning declared resource '{}' failed: {e:#}",
                    provider.name()
                )
            })
            .at(FailureStage::Provision)?;
        env_vars.extend(vars);
    }

    Ok(env_vars)
}

pub async fn read_kennel_config(
    config_store_path: &str,
) -> anyhow::Result<kennel_config::KennelConfig> {
    let json_path = std::path::Path::new(config_store_path).join("kennel.json");
    let content = tokio::fs::read_to_string(&json_path).await?;
    Ok(serde_json::from_str(&content)?)
}

/// Substitutes a store path from the binary cache
pub async fn realise(store_path: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("nix-store")
        .args(["--realise", store_path])
        .output()
        .await?;

    anyhow::ensure!(
        output.status.success(),
        "nix-store --realise {store_path} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub async fn deploy_request(
    state: &AppState,
    request: &::entity::deploy_requests::Model,
    artifact: &::entity::artifacts::Model,
) -> anyhow::Result<DeployOutcome> {
    let project = state
        .store
        .projects()
        .find_by_id(&request.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {} not found", request.project_id))?;

    let environment = Environment::from_branch(&request.branch).unwrap_or(Environment::Dev);

    realise(&artifact.store_path)
        .await
        .at(FailureStage::Fetch)?;

    let config_store_path = artifact.store_path.as_str();
    let kennel_config = read_kennel_config(config_store_path)
        .await
        .at(FailureStage::Config)?;

    if environment == Environment::Preview && !kennel_config.preview_deployments {
        return Ok(DeployOutcome::SkippedPreview);
    }

    let branch_slug = sanitize(&request.branch);
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    let existing = state
        .store
        .deployments()
        .find_by_project_branch(&request.project_id, &request.branch)
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
        project: &project,
        commit_sha: &request.commit_sha,
        config_store_path,
        branch: &request.branch,
        branch_slug: &branch_slug,
        environment: &environment,
        kennel_config: &kennel_config,
    };

    let deploy_result: Result<(), StageError> = async {
        for (name, site_config) in &kennel_config.static_sites {
            let store_path = site_config
                .store_path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("site '{name}' has no store path"))
                .at(FailureStage::Config)?;
            deploy_static_site(&ctx, name, store_path, site_config).await?;
        }

        for (name, svc_config) in &kennel_config.services {
            let store_path = svc_config
                .store_path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("service '{name}' has no store path"))
                .at(FailureStage::Config)?;
            deploy_service(&ctx, name, store_path, svc_config).await?;
        }
        Ok(())
    }
    .await;

    // Pre-start failures have no unit logs of their own
    if let Err(e) = &deploy_result {
        tracing::error!(
            project = %project.name,
            branch = %request.branch,
            commit = %request.commit_sha,
            stage = e.stage.as_str(),
            error = %format!("{e:#}"),
            "deploy failed",
        );
    }

    if let Some(owner) = project.owner.as_deref() {
        let (status, description) = match &deploy_result {
            Ok(()) => ("success", "deployment healthy".to_string()),
            Err(e) => ("failure", format!("{e:#}")),
        };
        // Static sites write no logs
        let has_logs = !kennel_config.services.is_empty() || deploy_result.is_err();
        let target_url = state
            .config
            .grafana_url
            .as_deref()
            .filter(|_| has_logs)
            .map(|base| deploy_logs_url(base, &project.name, &request.branch, &request.commit_sha));

        let posted = state
            .forgejo
            .create_commit_status(
                owner,
                &project.name,
                &request.commit_sha,
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
            tracing::error!(project = %project.name, error = %post_err, "failed to post commit status");
        }
    }

    deploy_result?;

    if let Err(e) = state.store.artifacts().prune_superseded(artifact).await {
        tracing::warn!(project = %project.name, branch = %request.branch, error = %e, "failed to prune superseded artifacts");
    }

    if let Some(pr_number) = crate::forgejo::pr_number_from_branch(&request.branch)
        && let Err(e) = post_pr_deployment_comment(
            state,
            &project,
            &request.branch,
            &request.commit_sha,
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
) -> Result<(), StageError> {
    let &DeployCtx {
        state,
        caddy,
        project,
        commit_sha,
        config_store_path,
        branch,
        branch_slug,
        environment,
        kennel_config: _,
    } = ctx;
    let project_name = project.name.as_str();
    let (domain, custom_domain) = deployment_domains(
        project_name,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
        site_config.custom_domain.as_deref(),
        environment,
    );

    let existing = state
        .store
        .deployments()
        .find_by_project_service_branch(&project.id, name, branch)
        .await
        .at(FailureStage::Internal)?;
    let deployment_id = existing
        .as_ref()
        .map_or_else(|| uuid::Uuid::now_v7().to_string(), |d| d.id.clone());

    let site_dir = PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
        .join(&project.id)
        .join(branch_slug);
    tokio::fs::create_dir_all(&site_dir)
        .await
        .at(FailureStage::Internal)?;

    let link = site_dir.join(name);
    let temp = site_dir.join(format!("{name}.new"));
    let _ = tokio::fs::remove_file(&temp).await;

    #[cfg(unix)]
    tokio::fs::symlink(store_path, &temp)
        .await
        .at(FailureStage::Internal)?;

    if link.exists() {
        tokio::fs::remove_file(&link)
            .await
            .at(FailureStage::Internal)?;
    }
    tokio::fs::rename(&temp, &link)
        .await
        .at(FailureStage::Internal)?;

    let public = match std::env::var("VAULT_ENDPOINT") {
        Ok(vault_endpoint) => crate::secrets::resolve_public(
            std::path::Path::new(config_store_path),
            &environment.to_string(),
            &vault_endpoint,
            project.secret_namespace.as_deref(),
        )
        .map_err(|e| StageError {
            stage: FailureStage::Secrets,
            error: anyhow::anyhow!("public value resolution failed for site '{name}': {e}"),
        })?,
        Err(_) => HashMap::new(),
    };

    let env_dir = site_env_dir(&project.id, branch_slug, name);
    write_site_env(&env_dir, &public)
        .await
        .at(FailureStage::Internal)?;
    let env_dir = env_dir.to_string_lossy();

    let route_id = format!("kennel-{deployment_id}");

    caddy
        .add_static_route(&route_id, &domain, store_path, &env_dir, site_config.spa)
        .await
        .at(FailureStage::Routing)?;

    if let Some(custom_domain) = custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_static_route(
                &custom_route_id,
                custom_domain,
                store_path,
                &env_dir,
                site_config.spa,
            )
            .await
            .at(FailureStage::Routing)?;
        ensure_dns_record(state, project_name, custom_domain).await;
    }

    route_www_redirect(
        state,
        caddy,
        project_name,
        &deployment_id,
        custom_domain,
        existing.as_ref().and_then(|d| d.custom_domain.as_deref()),
    )
    .await
    .at(FailureStage::Routing)?;

    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id.clone()),
        project_id: Set(project.id.clone()),
        service_name: Set(name.to_string()),
        service_type: Set("static".to_string()),
        branch: Set(branch.to_string()),
        branch_slug: Set(branch_slug.to_string()),
        environment: Set(environment.to_string()),
        commit_sha: Set(commit_sha.to_string()),
        store_path: Set(store_path.to_string()),
        domain: Set(domain.clone()),
        custom_domain: Set(custom_domain.map(str::to_string)),
        spa: Set(site_config.spa),
        ..Default::default()
    };

    state
        .store
        .deployments()
        .upsert(model)
        .await
        .at(FailureStage::Internal)?;
    add_gc_root(&deployment_id, store_path)
        .await
        .at(FailureStage::Internal)?;

    tracing::info!(site = %name, domain = %domain, "deployed static site");
    Ok(())
}

/// Paths a site's runtime public values are served at, in SvelteKit's format and as JSON
pub const SITE_ENV_FILES: [&str; 2] = ["/_app/env.js", "/_kennel/env.json"];

pub fn site_env_dir(project_id: &str, branch_slug: &str, name: &str) -> PathBuf {
    PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
        .join(project_id)
        .join(branch_slug)
        .join(format!("{name}.env"))
}

async fn write_site_env(
    dir: &std::path::Path,
    values: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(values)?;

    tokio::fs::create_dir_all(dir.join("_app")).await?;
    tokio::fs::create_dir_all(dir.join("_kennel")).await?;

    tokio::fs::write(dir.join("_app/env.js"), format!("export const env={json}")).await?;
    tokio::fs::write(dir.join("_kennel/env.json"), json).await?;
    Ok(())
}

async fn deploy_service(
    ctx: &DeployCtx<'_>,
    name: &str,
    store_path: &str,
    svc_config: &kennel_config::ServiceConfig,
) -> Result<(), StageError> {
    let &DeployCtx {
        state,
        caddy,
        project,
        commit_sha,
        config_store_path,
        branch,
        branch_slug,
        environment,
        kennel_config,
    } = ctx;
    let project_name = project.name.as_str();
    let (domain, custom_domain) = deployment_domains(
        project_name,
        name,
        branch_slug,
        &state.config.ephemeral_domain,
        svc_config.custom_domain.as_deref(),
        environment,
    );
    let unit_name = service_unit_name(project_name, branch_slug, name);
    let system_user = service_user(&unit_name);

    let request = ResourceRequest {
        project_name: project.id.clone(),
        service_name: name.to_string(),
        branch_slug: branch_slug.to_string(),
        environment: *environment,
        system_user: system_user.clone(),
    };

    let mut env_vars = svc_config.env.clone();
    env_vars.extend(provision_declared(state, kennel_config, &request).await?);

    if let Ok(vault_endpoint) = std::env::var("VAULT_ENDPOINT") {
        let env_str = environment.to_string();
        match crate::secrets::resolve(
            std::path::Path::new(config_store_path),
            &env_str,
            &vault_endpoint,
            project.secret_namespace.as_deref(),
        ) {
            Ok(secrets) => env_vars.extend(secrets),
            Err(e) => {
                return Err(StageError {
                    stage: FailureStage::Secrets,
                    error: anyhow::anyhow!("secret resolution failed for service '{name}': {e}"),
                });
            }
        }
    }

    let exec_start = match &svc_config.exec {
        Some(exec) => exec.clone(),
        None => find_executable(store_path)
            .await
            .at(FailureStage::Internal)?,
    };
    let port = allocate_port(&unit_name);
    env_vars.insert("PORT".to_string(), port.to_string());
    env_vars.insert("COMMIT_HASH".to_string(), commit_sha.to_string());

    env_vars
        .entry("APP_URL".to_string())
        .or_insert_with(|| app_url(&domain, custom_domain));

    let existing = state
        .store
        .deployments()
        .find_by_project_service_branch(&project.id, name, branch)
        .await
        .at(FailureStage::Internal)?;
    let deployment_id = existing
        .as_ref()
        .map_or_else(|| uuid::Uuid::now_v7().to_string(), |d| d.id.clone());

    let systemd = crate::systemd::SystemdClient::connect()
        .await
        .at(FailureStage::Systemd)?;
    systemd
        .start_transient_unit(
            &unit_name,
            &exec_start,
            &env_vars,
            &system_user,
            Some(config_store_path),
            &deploy_log_fields(project_name, branch, commit_sha),
        )
        .await
        .at(FailureStage::Systemd)?;

    // Wait for the service to become healthy before routing traffic
    let healthy = wait_for_healthy(port).await;
    if !healthy {
        tracing::error!(service = %name, port, "healthcheck failed, stopping unit");
        let _ = systemd.stop_unit(&unit_name).await;
        return Err(StageError {
            stage: FailureStage::Healthcheck,
            error: anyhow::anyhow!(
                "service '{name}' failed healthcheck within startup grace period"
            ),
        });
    }

    let route_id = format!("kennel-{deployment_id}");

    caddy
        .add_proxy_route(&route_id, &domain, port)
        .await
        .at(FailureStage::Routing)?;

    if let Some(custom_domain) = custom_domain {
        let custom_route_id = format!("kennel-{deployment_id}-custom");
        caddy
            .add_proxy_route(&custom_route_id, custom_domain, port)
            .await
            .at(FailureStage::Routing)?;
        ensure_dns_record(state, project_name, custom_domain).await;
    }

    route_www_redirect(
        state,
        caddy,
        project_name,
        &deployment_id,
        custom_domain,
        existing.as_ref().and_then(|d| d.custom_domain.as_deref()),
    )
    .await
    .at(FailureStage::Routing)?;

    // Commit before moving the gc root so the recorded path is never unrooted
    let model = ::entity::deployments::ActiveModel {
        id: Set(deployment_id.clone()),
        project_id: Set(project.id.clone()),
        service_name: Set(name.to_string()),
        service_type: Set("service".to_string()),
        branch: Set(branch.to_string()),
        branch_slug: Set(branch_slug.to_string()),
        environment: Set(environment.to_string()),
        commit_sha: Set(commit_sha.to_string()),
        store_path: Set(store_path.to_string()),
        domain: Set(domain.clone()),
        custom_domain: Set(custom_domain.map(str::to_string)),
        spa: Set(false),
        unit_name: Set(Some(unit_name)),
        port: Set(Some(i64::from(port))),
        config_store_path: Set(Some(config_store_path.to_string())),
        ..Default::default()
    };

    state
        .store
        .deployments()
        .upsert(model)
        .await
        .at(FailureStage::Internal)?;

    // Pin the new artifact under the deployment root
    add_gc_root(&deployment_id, store_path)
        .await
        .at(FailureStage::Internal)?;
    add_gc_root(&format!("{deployment_id}-config"), config_store_path)
        .await
        .at(FailureStage::Internal)?;

    tracing::info!(service = %name, domain = %domain, port = port, "deployed service");
    Ok(())
}

/// Upserts the proxied Cloudflare CNAME routing a custom domain through the tunnel
pub async fn ensure_dns_record(state: &AppState, project_name: &str, fqdn: &str) {
    let Some(cf) = &state.cloudflare else {
        return;
    };

    match cf.upsert_record(project_name, fqdn).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(fqdn = %fqdn, "custom domain outside configured cloudflare zones, it will not resolve");
        }
        Err(e) => {
            tracing::warn!(fqdn = %fqdn, error = %e, "failed to upsert cloudflare record");
        }
    }
}

pub async fn delete_dns_record(state: &AppState, fqdn: &str) {
    let Some(cf) = &state.cloudflare else {
        return;
    };

    if let Err(e) = cf.delete_record(fqdn).await {
        tracing::warn!(fqdn = %fqdn, error = %e, "failed to delete cloudflare record");
    }
}

/// Redirects `www.{custom_domain}` to an apex custom domain
async fn route_www_redirect(
    state: &AppState,
    caddy: &CaddyClient,
    project_name: &str,
    deployment_id: &str,
    custom_domain: Option<&str>,
    previous_custom_domain: Option<&str>,
) -> anyhow::Result<()> {
    let route_id = format!("kennel-{deployment_id}-redirect");
    let redirect = custom_domain.and_then(www_redirect_domain);
    let previous = previous_custom_domain.and_then(www_redirect_domain);

    match redirect.as_deref().zip(custom_domain) {
        Some((from, target)) => {
            caddy.add_redirect_route(&route_id, from, target).await?;
            ensure_dns_record(state, project_name, from).await;
        }
        None if previous.is_some() => caddy.remove_route(&route_id).await?,
        None => {}
    }

    if let Some(previous) = previous
        && redirect.as_ref() != Some(&previous)
        && custom_domain != Some(previous.as_str())
    {
        delete_dns_record(state, &previous).await;
    }

    Ok(())
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

/// Whether a deployment's artifact is still present in the nix store
pub async fn store_path_exists(store_path: &str) -> bool {
    tokio::fs::metadata(store_path).await.is_ok()
}

/// Whether the gc root symlink at `root` already resolves to `target`
async fn root_points_at(root: &std::path::Path, target: &str) -> bool {
    tokio::fs::read_link(root)
        .await
        .map(|dest| dest.to_string_lossy() == target)
        .unwrap_or(false)
}

/// Re-pin a live deployment's gc roots to its recorded store path
pub async fn ensure_gc_root(deployment: &::entity::deployments::Model) {
    let dir = PathBuf::from(kennel_config::constants::GC_ROOTS_DIR);

    if !root_points_at(&dir.join(&deployment.id), &deployment.store_path).await
        && let Err(e) = add_gc_root(&deployment.id, &deployment.store_path).await
    {
        tracing::warn!(deployment = %deployment.id, error = %e, "failed to re-pin gc root");
    }

    if let Some(config_store_path) = &deployment.config_store_path {
        let name = format!("{}-config", deployment.id);
        if !root_points_at(&dir.join(&name), config_store_path).await
            && let Err(e) = add_gc_root(&name, config_store_path).await
        {
            tracing::warn!(deployment = %deployment.id, error = %e, "failed to re-pin config gc root");
        }
    }
}

/// Substitutes a garbage-collected artifact again, stopping the unit if that fails
pub async fn recover_collected_artifact(
    deployment: &::entity::deployments::Model,
    systemd: &crate::systemd::SystemdClient,
) -> bool {
    match realise(&deployment.store_path).await {
        Ok(()) => {
            tracing::warn!(
                deployment = %deployment.id,
                store_path = %deployment.store_path,
                "artifact was missing from the store; fetched it again",
            );
            true
        }
        Err(e) => {
            if let Some(unit_name) = &deployment.unit_name {
                let _ = systemd.stop_unit(unit_name).await;
            }

            tracing::error!(
                deployment = %deployment.id,
                store_path = %deployment.store_path,
                error = %e,
                "artifact missing from the store and the binary cache; stopped unit until the next push",
            );
            false
        }
    }
}

pub async fn remove_gc_roots(deployment_id: &str) {
    let dir = PathBuf::from(kennel_config::constants::GC_ROOTS_DIR);
    let _ = tokio::fs::remove_file(dir.join(deployment_id)).await;
    let _ = tokio::fs::remove_file(dir.join(format!("{deployment_id}-config"))).await;
}

/// Generated domain of a deployment, and its custom domain if it is in prod
pub fn deployment_domains<'a>(
    project: &str,
    service: &str,
    branch_slug: &str,
    base_domain: &str,
    custom_domain: Option<&'a str>,
    environment: &Environment,
) -> (String, Option<&'a str>) {
    let domain = format!("{project}-{service}-{branch_slug}.{base_domain}");
    let custom_domain = custom_domain.filter(|_| *environment == Environment::Prod);
    (domain, custom_domain)
}

/// Public URL of a deployment, set as `APP_URL` in its services
pub fn app_url(domain: &str, custom_domain: Option<&str>) -> String {
    format!("https://{}", custom_domain.unwrap_or(domain))
}

/// The www name redirected to a `name.tld` custom domain
pub fn www_redirect_domain(custom_domain: &str) -> Option<String> {
    (custom_domain.matches('.').count() == 1).then(|| format!("www.{custom_domain}"))
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

pub fn service_unit_name(project_name: &str, branch_slug: &str, service: &str) -> String {
    format!(
        "kennel-{}-{}-{}",
        sanitize(project_name),
        branch_slug,
        sanitize(service)
    )
}

/// Loki's full 720h retention
const LOG_RANGE_PARAMS: &str = "&from=now-30d&to=now";

/// `LogExtraFields` for a service unit, kept across restarts
pub fn deploy_log_fields(project_name: &str, branch: &str, commit_sha: &str) -> Vec<String> {
    vec![
        format!("KENNEL_PROJECT={project_name}"),
        format!("KENNEL_BRANCH={branch}"),
        format!("KENNEL_COMMIT={commit_sha}"),
    ]
}

pub fn deploy_logs_url(base: &str, project_name: &str, branch: &str, commit_sha: &str) -> String {
    drilldown_project_url(
        base,
        project_name,
        &[("branch", branch), ("commit", commit_sha)],
    )
}

/// Filters on the `project` label and per-line metadata Alloy extracts from `KENNEL_*` fields
fn drilldown_project_url(base: &str, project_name: &str, metadata: &[(&str, &str)]) -> String {
    let base = base.trim_end_matches('/');
    let project_path = urlencoding::encode(project_name);
    let project_filter = urlencoding::encode(&format!("project|=|{project_name}")).into_owned();

    let mut url = format!(
        "{base}/a/grafana-lokiexplore-app/explore/project/{project_path}/logs?patterns=%5B%5D&var-ds=loki&var-filters={project_filter}"
    );
    for (key, value) in metadata {
        url.push_str("&var-metadata=");
        url.push_str(&urlencoding::encode(&format!("{key}|=|{value}")));
    }

    url.push_str(LOG_RANGE_PARAMS);
    url
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
