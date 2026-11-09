use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use kennel_config::api;
use std::sync::Arc;

use crate::AppState;
use crate::systemd::SystemdClient;

pub async fn health(
    State(state): State<Arc<AppState>>,
    Path(deployment_id): Path<String>,
) -> Result<Json<api::Health>, StatusCode> {
    let deployment = state
        .store
        .deployments()
        .find_by_id(&deployment_id)
        .await
        .map_err(internal)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if deployment.unit_name.is_none() {
        return Err(StatusCode::CONFLICT);
    }

    let systemd = SystemdClient::connect().await.map_err(internal)?;
    unit_health(&systemd, &deployment)
        .await
        .map(Json)
        .map_err(internal)
}

/// Serves `GET /projects/{project}/branches/{branch}`
pub async fn branch(
    State(state): State<Arc<AppState>>,
    Path((project, branch)): Path<(String, String)>,
) -> Result<Json<api::Branch>, StatusCode> {
    let project = state
        .store
        .projects()
        .find_by_name(&project)
        .await
        .map_err(internal)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let request = state
        .store
        .deploy_requests()
        .find_by_project_branch(&project.id, &branch)
        .await
        .map_err(internal)?;
    let deployments = state
        .store
        .deployments()
        .find_by_project_branch(&project.id, &branch)
        .await
        .map_err(internal)?;

    let request = request
        .map(|request| {
            Ok::<_, StatusCode>(api::Request {
                status: request_status(&request.status).ok_or_else(|| {
                    internal(format!("unknown deploy request status {}", request.status))
                })?,
                commit: request.commit_sha,
                failure_stage: request.failure_stage,
            })
        })
        .transpose()?;

    let systemd = SystemdClient::connect().await.map_err(internal)?;
    let mut entries = Vec::with_capacity(deployments.len());
    for deployment in deployments {
        let health = match deployment.unit_name {
            Some(_) => unit_health(&systemd, &deployment).await.ok(),
            None => None,
        };
        entries.push(api::Deployment {
            url: format!("https://{}", deployment.domain),
            custom_url: deployment
                .custom_domain
                .map(|domain| format!("https://{domain}")),
            id: deployment.id,
            name: deployment.service_name,
            kind: deployment.service_type,
            commit: deployment.commit_sha,
            health,
        });
    }

    Ok(Json(api::Branch {
        project: project.name,
        branch,
        request,
        deployments: entries,
    }))
}

async fn unit_health(
    systemd: &SystemdClient,
    deployment: &::entity::deployments::Model,
) -> anyhow::Result<api::Health> {
    let unit_name = deployment
        .unit_name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("deployment {} has no unit", deployment.id))?;
    let mut health = systemd.get_health(unit_name).await?;

    if health.active
        && let Some(port) = deployment.port
    {
        health.app_healthy = Some(crate::health::probe(port as u16).await);
    }
    Ok(health)
}

fn request_status(status: &str) -> Option<api::RequestStatus> {
    match status {
        "pending" => Some(api::RequestStatus::Pending),
        "deployed" => Some(api::RequestStatus::Deployed),
        "failed" => Some(api::RequestStatus::Failed),
        "skipped" => Some(api::RequestStatus::Skipped),
        _ => None,
    }
}

fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::error!(error = %e, "deployments handler failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
