use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;

use crate::AppState;
use crate::systemd::SystemdClient;

pub async fn health(
    State(state): State<Arc<AppState>>,
    Path(deployment_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let deployment = state
        .store
        .deployments()
        .find_by_id(&deployment_id)
        .await
        .map_err(internal)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let unit_name = deployment.unit_name.ok_or(StatusCode::CONFLICT)?;

    let systemd = SystemdClient::connect().await.map_err(internal)?;
    let mut h = systemd.get_health(&unit_name).await.map_err(|e| {
        tracing::error!(unit = %unit_name, error = %e, "failed to query unit health");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if h.active
        && let Some(port) = deployment.port
    {
        h.app_healthy = Some(crate::health::probe(port as u16).await);
    }

    Ok(Json(h))
}

fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::error!(error = %e, "deployments handler failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
