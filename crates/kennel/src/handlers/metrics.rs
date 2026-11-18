use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use std::fmt::Write;
use std::sync::Arc;

use crate::AppState;
use crate::deploy::FailureStage;

pub async fn scrape(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, StatusCode> {
    let request_counts = state
        .store
        .deploy_requests()
        .count_by_status()
        .await
        .map_err(internal)?;
    let deployments_count = state.store.deployments().count().await.map_err(internal)?;
    let projects_count = state.store.projects().count().await.map_err(internal)?;
    let failed_by_stage = state
        .store
        .deploy_requests()
        .count_failed_by_stage()
        .await
        .map_err(internal)?;

    let mut out = String::new();

    writeln!(
        out,
        "# HELP kennel_deploy_requests Branch deploy intents grouped by status"
    )
    .map_err(internal)?;
    writeln!(out, "# TYPE kennel_deploy_requests gauge").map_err(internal)?;
    for (status, count) in &request_counts {
        writeln!(out, "kennel_deploy_requests{{status=\"{status}\"}} {count}").map_err(internal)?;
    }

    writeln!(
        out,
        "# HELP kennel_deployments Active deployments tracked by kennel"
    )
    .map_err(internal)?;
    writeln!(out, "# TYPE kennel_deployments gauge").map_err(internal)?;
    writeln!(out, "kennel_deployments {deployments_count}").map_err(internal)?;

    writeln!(
        out,
        "# HELP kennel_projects Projects registered with kennel"
    )
    .map_err(internal)?;
    writeln!(out, "# TYPE kennel_projects gauge").map_err(internal)?;
    writeln!(out, "kennel_projects {projects_count}").map_err(internal)?;

    writeln!(
        out,
        "# HELP kennel_deploy_failures Deploy requests currently failed, by the stage that failed"
    )
    .map_err(internal)?;
    writeln!(out, "# TYPE kennel_deploy_failures gauge").map_err(internal)?;
    // Every stage is emitted so an alert on one stage never sees missing data
    for stage in FailureStage::ALL {
        let label = stage.as_str();
        let count = failed_by_stage
            .iter()
            .find(|(s, _)| s == label)
            .map_or(0, |(_, n)| *n);
        writeln!(out, "kennel_deploy_failures{{stage=\"{label}\"}} {count}").map_err(internal)?;
    }

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
