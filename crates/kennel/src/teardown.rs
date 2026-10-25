use crate::AppState;
use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use kennel_provision::{ResourceProvider, ResourceRequest};

/// Tear down a single deployment by stopping its unit, removing its caddy
/// route and DNS record, deprovisioning resources, and deleting the row.
pub async fn teardown_deployment(
    state: &AppState,
    deployment: &::entity::deployments::Model,
    systemd: &SystemdClient,
    caddy: &CaddyClient,
) -> anyhow::Result<()> {
    let route_id = format!("kennel-{}", deployment.id);

    // Stop systemd unit if this is a service deployment
    if let Some(ref unit_name) = deployment.unit_name
        && systemd.is_active(unit_name).await
        && let Err(e) = systemd.stop_unit(unit_name).await
    {
        tracing::warn!(unit = %unit_name, error = %e, "failed to stop unit");
    }

    // Remove static site symlink and runtime env
    if deployment.service_type == "static" {
        let link = std::path::PathBuf::from(kennel_config::constants::SITES_BASE_DIR)
            .join(&deployment.project_id)
            .join(&deployment.branch_slug)
            .join(&deployment.service_name);
        let _ = tokio::fs::remove_file(&link).await;
        let _ = tokio::fs::remove_dir_all(crate::deploy::site_env_dir(
            &deployment.project_id,
            &deployment.branch_slug,
            &deployment.service_name,
        ))
        .await;
    }

    // Remove caddy routes
    if let Err(e) = caddy.remove_route(&route_id).await {
        tracing::warn!(route = %route_id, error = %e, "failed to remove caddy route");
    }

    if let Some(ref custom_domain) = deployment.custom_domain {
        let custom_route_id = format!("kennel-{}-custom", deployment.id);
        if let Err(e) = caddy.remove_route(&custom_route_id).await {
            tracing::warn!(route = %custom_route_id, error = %e, "failed to remove custom caddy route");
        }
        crate::deploy::delete_dns_record(state, custom_domain).await;
    }

    if let Some(redirect_domain) = deployment
        .custom_domain
        .as_deref()
        .and_then(crate::deploy::www_redirect_domain)
    {
        let redirect_route_id = format!("kennel-{}-redirect", deployment.id);
        if let Err(e) = caddy.remove_route(&redirect_route_id).await {
            tracing::warn!(route = %redirect_route_id, error = %e, "failed to remove redirect caddy route");
        }
        crate::deploy::delete_dns_record(state, &redirect_domain).await;
    }

    // Remove GC roots
    crate::deploy::remove_gc_roots(&deployment.id).await;

    // Deprovision resources
    let request = ResourceRequest {
        project_name: deployment.project_id.clone(),
        service_name: deployment.service_name.clone(),
        branch_slug: deployment.branch_slug.clone(),
        environment: kennel_config::Environment::from_branch(&deployment.branch)
            .unwrap_or(kennel_config::Environment::Dev),
        system_user: deployment
            .unit_name
            .as_deref()
            .map(crate::deploy::service_user)
            .unwrap_or_default(),
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
