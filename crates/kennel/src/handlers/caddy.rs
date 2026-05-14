use axum::{
    extract::{Query, State},
    http::StatusCode,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

pub async fn check_domain(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> StatusCode {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    match state.store.deployments().find_by_domain(domain).await {
        Ok(Some(_)) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}
