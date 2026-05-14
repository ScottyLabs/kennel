use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use std::sync::Arc;

use crate::AppState;

pub async fn log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let build = state
        .store
        .builds()
        .find_by_id(&id)
        .await
        .map_err(|e| {
            tracing::error!(build_id = %id, error = %e, "failed to look up build");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    Ok((headers, build.log.unwrap_or_default()))
}
