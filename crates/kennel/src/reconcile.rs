use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use crate::teardown::teardown_deployment;
use crate::{AppState, deploy};
use chrono::Utc;
use kennel_provision::ResourceProvider;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run_once(
    state: &AppState,
    restart_failures: &mut HashMap<String, u32>,
) -> anyhow::Result<()> {
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

    // Restart service deployments whose units are not running
    for deployment in &deployments {
        let Some(ref unit_name) = deployment.unit_name else {
            continue;
        };

        if !systemd.is_active(unit_name).await {
            let fail_count = restart_failures.entry(unit_name.clone()).or_insert(0);
            if *fail_count >= kennel_config::constants::HEALTHCHECK_FAILURE_THRESHOLD {
                tracing::warn!(
                    unit = %unit_name,
                    failures = *fail_count,
                    "unit crash-looping, skipping restart"
                );
                continue;
            }
            *fail_count += 1;

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

            let mut env_vars: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for provider in &state.providers {
                if let Ok(vars) = provider.provision(&request).await {
                    env_vars.extend(vars);
                }
            }

            if let Ok(vault_endpoint) = std::env::var("VAULT_ENDPOINT")
                && let Some(ref config_store_path) = deployment.config_store_path
            {
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

            let exec = deploy::find_executable(&deployment.store_path).await;
            if let Ok(exec_start) = exec
                && let Err(e) = systemd
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
        } else {
            // Reset the failure counter once the unit is healthy
            restart_failures.remove(unit_name);
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
    let mut restart_failures: HashMap<String, u32> = HashMap::new();

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = state.signal.notified() => {}
            _ = cancel.cancelled() => break,
        }

        if let Err(e) = run_once(&state, &mut restart_failures).await {
            tracing::error!(error = %e, "reconciliation failed");
        }
    }
}
