use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use crate::{deploy, AppState};
use kennel_provision::ResourceProvider;
use std::collections::HashSet;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run_once(state: &AppState) -> anyhow::Result<()> {
    let systemd = SystemdClient::connect().await?;
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    let reset = state.store.builds().reset_stuck().await?;
    if reset > 0 {
        tracing::info!(count = reset, "reset stuck builds to queued");
    }

    // Deploy completed builds that haven't been deployed yet
    let built = state.store.builds().find_by_status("built").await?;
    for build in &built {
        match deploy::deploy_build(state, build).await {
            Ok(()) => {
                let _ = state.store.builds().set_status(&build.id, "done").await;
            }
            Err(e) => {
                tracing::error!(build_id = %build.id, error = %e, "deploy failed during reconciliation");
                let _ = state.store.builds().set_status(&build.id, "failed").await;
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
            tracing::info!(unit = %unit_name, "restarting missing unit");

            let request = kennel_provision::ResourceRequest {
                project_name: deployment.project_id.clone(),
                service_name: deployment.service_name.clone(),
                branch_slug: deployment.branch_slug.clone(),
                environment: kennel_config::Environment::from_branch(&deployment.branch),
            };

            let mut env_vars: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for provider in &state.providers {
                if let Ok(vars) = provider.provision(&request).await {
                    env_vars.extend(vars);
                }
            }

            if let Ok(vault_endpoint) = dotenvy::var("VAULT_ENDPOINT") {
                let env_str = request.environment.to_string();
                if let Ok(secrets) = crate::secrets::resolve(
                    std::path::Path::new(&deployment.store_path),
                    &env_str,
                    &vault_endpoint,
                ) {
                    env_vars.extend(secrets);
                }
            }

            if let Some(port) = deployment.port {
                env_vars.insert("PORT".to_string(), port.to_string());
            }

            let exec = deploy::find_executable(&deployment.store_path).await;
            if let Ok(exec_start) = exec {
                if let Err(e) = systemd
                    .start_transient_unit(unit_name, &exec_start, Some(unit_name), &env_vars)
                    .await
                {
                    tracing::error!(unit = %unit_name, error = %e, "failed to restart unit");
                }
            }
        }
    }

    // Caddy does not persist routes across restarts, so re-add all of them
    for deployment in &deployments {
        let route_id = format!("kennel-{}", deployment.id);

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

        if let Some(ref custom_domain) = deployment.custom_domain {
            let custom_route_id = format!("kennel-{}-custom", deployment.id);
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
    }

    tracing::info!(deployments = deployments.len(), "reconciliation complete");

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
