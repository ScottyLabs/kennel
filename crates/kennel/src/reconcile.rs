use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use crate::teardown::teardown_deployment;
use crate::{AppState, deploy};
use chrono::Utc;
use kennel_provision::ResourceProvider;
use std::collections::HashSet;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let systemd = SystemdClient::connect().await?;
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    // Deploy pending intents whose build artifacts are ready
    let pending = state
        .store
        .deploy_requests()
        .find_by_status("pending")
        .await?;
    for request in &pending {
        let Some(build) = state
            .store
            .builds()
            .find_by_project_commit(&request.project_id, &request.commit_sha)
            .await?
        else {
            continue;
        };
        match build.status.as_str() {
            "built" => match deploy::deploy_request(state, request, &build).await {
                Ok(deploy::DeployOutcome::Deployed) => {
                    let _ = state
                        .store
                        .deploy_requests()
                        .set_status(&request.id, "deployed")
                        .await;
                }
                Ok(deploy::DeployOutcome::SkippedPreview) => {
                    let _ = state
                        .store
                        .deploy_requests()
                        .set_status(&request.id, "skipped")
                        .await;
                }
                Err(e) => {
                    tracing::error!(request_id = %request.id, error = %e, "deploy failed during reconciliation");
                    let _ = state
                        .store
                        .deploy_requests()
                        .set_status(&request.id, "failed")
                        .await;
                }
            },
            "failed" | "cancelled" => {
                let _ = state
                    .store
                    .deploy_requests()
                    .set_status(&request.id, "failed")
                    .await;
            }
            _ => {}
        }
    }

    // Expire non-production deployments older than the expiry threshold
    let all_deployments = state.store.deployments().list_all().await?;
    let expiry_cutoff =
        Utc::now() - chrono::Duration::days(kennel_config::constants::DEPLOYMENT_EXPIRY_DAYS);
    for deployment in &all_deployments {
        let env = kennel_config::Environment::from_branch(&deployment.branch);
        if !matches!(
            env,
            Some(kennel_config::Environment::Prod) | Some(kennel_config::Environment::Staging)
        ) && deployment.updated_at < expiry_cutoff
        {
            tracing::info!(
                deployment = %deployment.id,
                domain = %deployment.domain,
                branch = %deployment.branch,
                "expiring stale deployment"
            );
            if let Err(e) = teardown_deployment(state, deployment, &systemd, &caddy).await {
                tracing::error!(deployment = %deployment.id, error = %e, "expiry teardown failed");
            }
        }
    }

    let deployments = state.store.deployments().list_all().await?;
    let desired_units: HashSet<String> = deployments
        .iter()
        .filter_map(|d| d.unit_name.clone())
        .collect();

    let actual_units: HashSet<String> = systemd
        .list_kennel_units()
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Stop units that have no corresponding deployment record
    for orphan in actual_units.difference(&desired_units) {
        tracing::warn!(unit = %orphan, "stopping orphaned unit");
        let _ = systemd.stop_unit(orphan).await;
    }

    // Re-pin each live gc root, rebuild collected artifacts, and restart down units
    for deployment in &deployments {
        if !deploy::store_path_exists(&deployment.store_path).await {
            deploy::recover_collected_artifact(state, deployment, &systemd).await;
            continue;
        }
        deploy::ensure_gc_root(deployment).await;

        let Some(unit_name) = &deployment.unit_name else {
            continue; // Static sites are served by Caddy, no unit to supervise
        };

        match systemd.get_health(unit_name).await {
            // Running or starting
            Ok(h)
                if matches!(
                    h.active_state.as_str(),
                    "active" | "activating" | "reloading"
                ) => {}
            // Crash-looped past the StartLimit so leave it failed
            Ok(h) if h.active_state == "failed" => {
                tracing::warn!(
                    unit = %unit_name,
                    result = %h.result,
                    restarts = h.n_restarts,
                    "unit crash-looping; leaving stopped until redeployed",
                );
            }
            // Down for a benign reason like a reboot so restart it
            _ => restart_service(state, &systemd, deployment).await,
        }
    }

    // Caddy does not persist dynamic routes, re-add the missing ones
    let existing_routes = caddy.list_route_ids().await.unwrap_or_default();

    for deployment in &deployments {
        let route_id = format!("kennel-{}", deployment.id);

        if !existing_routes.contains(&route_id) {
            if deployment.service_type == "static" {
                let _ = caddy
                    .add_static_route(
                        &route_id,
                        &deployment.domain,
                        &deployment.store_path,
                        deployment.spa,
                    )
                    .await;
            } else if let Some(port) = deployment.port {
                let _ = caddy
                    .add_proxy_route(&route_id, &deployment.domain, port as u16)
                    .await;
            }
        }

        if let Some(ref custom_domain) = deployment.custom_domain {
            let custom_route_id = format!("kennel-{}-custom", deployment.id);
            if !existing_routes.contains(&custom_route_id) {
                if deployment.service_type == "static" {
                    let _ = caddy
                        .add_static_route(
                            &custom_route_id,
                            custom_domain,
                            &deployment.store_path,
                            deployment.spa,
                        )
                        .await;
                } else if let Some(port) = deployment.port {
                    let _ = caddy
                        .add_proxy_route(&custom_route_id, custom_domain, port as u16)
                        .await;
                }
            }

            let project_name = state
                .store
                .projects()
                .find_by_id(&deployment.project_id)
                .await
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_else(|| deployment.project_id.clone());
            deploy::ensure_dns_record(state, &project_name, custom_domain).await;
        }
    }

    // Reconcile provisioned resources against active deployments
    let active_requests: Vec<kennel_provision::ResourceRequest> = deployments
        .iter()
        .map(|d| kennel_provision::ResourceRequest {
            project_name: d.project_id.clone(),
            service_name: d.service_name.clone(),
            branch_slug: d.branch_slug.clone(),
            environment: kennel_config::Environment::from_branch(&d.branch)
                .unwrap_or(kennel_config::Environment::Dev),
            system_user: d
                .unit_name
                .as_deref()
                .map(deploy::service_user)
                .unwrap_or_default(),
        })
        .collect();

    for provider in &state.providers {
        if let Err(e) = provider.reconcile(&active_requests).await {
            tracing::warn!(provider = provider.name(), error = %e, "resource reconcile failed");
        }
    }

    if let Err(e) = crate::custom_domains::publish(state).await {
        tracing::warn!(error = %e, "failed to publish custom domains");
    }

    tracing::debug!(deployments = deployments.len(), "reconciliation complete");

    Ok(())
}

pub async fn run_loop(state: Arc<AppState>, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(kennel_config::constants::RECONCILE_INTERVAL);

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = state.signal.notified() => {}
            _ = cancel.cancelled() => break,
        }

        if let Err(e) = run_once(&state).await {
            tracing::error!(error = %e, "reconciliation failed");
        }
    }
}

/// Relaunch a down service unit from its recorded state
async fn restart_service(
    state: &AppState,
    systemd: &SystemdClient,
    deployment: &::entity::deployments::Model,
) {
    let Some(unit_name) = &deployment.unit_name else {
        return;
    };

    tracing::info!(unit = %unit_name, "restarting missing unit");

    let system_user = deploy::service_user(unit_name);
    let request = kennel_provision::ResourceRequest {
        project_name: deployment.project_id.clone(),
        service_name: deployment.service_name.clone(),
        branch_slug: deployment.branch_slug.clone(),
        environment: kennel_config::Environment::from_branch(&deployment.branch)
            .unwrap_or(kennel_config::Environment::Dev),
        system_user: system_user.clone(),
    };

    let Some(config_store_path) = deployment.config_store_path.as_deref() else {
        tracing::warn!(unit = %unit_name, "no config store path, skipping restart");
        return;
    };
    let kennel_config = match deploy::read_kennel_config(config_store_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(unit = %unit_name, error = %e, "could not read kennel config, skipping restart");
            return;
        }
    };
    let mut env_vars = match deploy::provision_declared(state, &kennel_config, &request).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(unit = %unit_name, error = %e, "resource provisioning failed, skipping restart");
            return;
        }
    };

    if let Ok(vault_endpoint) = std::env::var("VAULT_ENDPOINT") {
        let env_str = request.environment.to_string();
        if let Ok(secrets) = crate::secrets::resolve(
            std::path::Path::new(config_store_path),
            &env_str,
            &vault_endpoint,
        ) {
            env_vars.extend(secrets);
        }
    }

    if let Some(port) = deployment.port {
        env_vars.insert("PORT".to_string(), port.to_string());
    }
    env_vars.insert("COMMIT_HASH".to_string(), deployment.commit_sha.clone());

    let app_url = format!(
        "https://{}",
        deployment
            .custom_domain
            .as_deref()
            .unwrap_or(deployment.domain.as_str())
    );
    env_vars.entry("APP_URL".to_string()).or_insert(app_url);

    match deploy::find_executable(&deployment.store_path).await {
        Ok(exec_start) => {
            if let Err(e) = systemd
                .start_transient_unit(
                    unit_name,
                    &exec_start,
                    &env_vars,
                    &system_user,
                    deployment.config_store_path.as_deref(),
                )
                .await
            {
                tracing::error!(unit = %unit_name, error = %e, "failed to restart unit");
            }
        }
        Err(e) => {
            tracing::error!(unit = %unit_name, error = %e, "could not find executable, skipping restart");
        }
    }
}
