use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{Response, StatusCode, header},
    response::IntoResponse,
};
use serde::Deserialize;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::io::ReaderStream;

use crate::AppState;
use crate::systemd::SystemdClient;

#[derive(Deserialize, Default)]
pub struct LogsParams {
    #[serde(default)]
    follow: bool,
    lines: Option<u32>,
    since: Option<String>,
}

pub async fn logs(
    State(state): State<Arc<AppState>>,
    Path(deployment_id): Path<String>,
    Query(params): Query<LogsParams>,
) -> Result<Response<Body>, StatusCode> {
    let deployment = state
        .store
        .deployments()
        .find_by_id(&deployment_id)
        .await
        .map_err(internal)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let unit_name = deployment.unit_name.ok_or(StatusCode::CONFLICT)?;

    let mut cmd = Command::new("journalctl");
    cmd.arg("-u").arg(format!("{unit_name}.service"));
    cmd.arg("-o").arg("json");
    cmd.arg("--no-pager");
    cmd.arg("--output-fields=__REALTIME_TIMESTAMP,_SYSTEMD_UNIT,PRIORITY,MESSAGE");

    if let Some(since) = &params.since {
        cmd.arg("--since").arg(since);
    }
    cmd.arg("-n").arg(params.lines.unwrap_or(500).to_string());
    if params.follow {
        cmd.arg("-f");
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        tracing::error!(error = %e, "failed to spawn journalctl");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(body)
        .map_err(internal)
}

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
    let h = systemd.get_health(&unit_name).await.map_err(|e| {
        tracing::error!(unit = %unit_name, error = %e, "failed to query unit health");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(h))
}

fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::error!(error = %e, "deployments handler failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
