use crate::AppState;
use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use kennel_provision::{Provider, ResourceProvider, ResourceRequest};

/// Tear down a single deployment: stop unit, remove route, deprovision resources, delete record.
pub async fn teardown_deployment(
    state: &AppState,
    deployment: &::entity::deployments::Model,
    systemd: &SystemdClient,
    caddy: &CaddyClient,
) -> anyhow::Result<()> {
    let route_id = format!("kennel-{}", deployment.id);

    // Stop systemd unit if this is a service deployment
    if let Some(ref unit_name) = deployment.unit_name {
        if systemd.is_active(unit_name).await {
            if let Err(e) = systemd.stop_unit(unit_name).await {
                tracing::warn!(unit = %unit_name, error = %e, "failed to stop unit");
            }
        }
    }

    // Remove static site symlink
    if deployment.service_type == "static" {
        let link = std::path::PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
            .join(&deployment.project_id)
            .join(&deployment.branch_slug)
            .join(&deployment.service_name);
        let _ = tokio::fs::remove_file(&link).await;
    }

    // Remove caddy route
    if let Err(e) = caddy.remove_route(&route_id).await {
        tracing::warn!(route = %route_id, error = %e, "failed to remove caddy route");
    }

    // Deprovision resources
    let request = ResourceRequest {
        project_name: deployment.project_id.clone(),
        service_name: deployment.service_name.clone(),
        branch_slug: deployment.branch_slug.clone(),
        environment: kennel_config::Environment::from_branch(&deployment.branch),
    };

    for provider in &state.providers {
        if let Err(e) = provider.teardown(&request).await {
            tracing::warn!(
                provider = provider.name(),
                error = %e,
                "resource teardown failed"
            );
        }
    }

    // Delete deployment record
    state.store.deployments().delete(&deployment.id).await?;

    tracing::info!(
        deployment = %deployment.id,
        domain = %deployment.domain,
        "torn down"
    );

    Ok(())
}
