use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use std::fmt::Write;
use std::sync::Arc;

use crate::AppState;

pub async fn scrape(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, StatusCode> {
    let build_counts = state
        .store
        .builds()
        .count_by_status()
        .await
        .map_err(internal)?;
    let deployments_count = state.store.deployments().count().await.map_err(internal)?;
    let projects_count = state.store.projects().count().await.map_err(internal)?;

    let mut out = String::new();

    writeln!(out, "# HELP kennel_builds Builds grouped by current status").ok();
    writeln!(out, "# TYPE kennel_builds gauge").ok();
    for (status, count) in &build_counts {
        writeln!(out, "kennel_builds{{status=\"{status}\"}} {count}").ok();
    }

    writeln!(
        out,
        "# HELP kennel_deployments Active deployments tracked by kennel"
    )
    .ok();
    writeln!(out, "# TYPE kennel_deployments gauge").ok();
    writeln!(out, "kennel_deployments {deployments_count}").ok();

    writeln!(
        out,
        "# HELP kennel_projects Projects registered with kennel"
    )
    .ok();
    writeln!(out, "# TYPE kennel_projects gauge").ok();
    writeln!(out, "kennel_projects {projects_count}").ok();

    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    Ok((headers, out))
}

fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::error!(error = %e, "metrics scrape failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
