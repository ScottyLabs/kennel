use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use entity::sea_orm_active_enums::BuildStatus;
use kennel_store::Store;
use sea_orm::{ActiveValue::Set, IntoActiveModel};
use std::sync::Arc;

#[utoipa::path(
    post,
    path = "/builds/{build_id}/cancel",
    params(("build_id" = i32, Path,)),
    responses(
        (status = OK, description = "Build cancelled successfully"),
        (status = NOT_FOUND, description = "Build not found"),
        (status = BAD_REQUEST, description = "Build cannot be cancelled"),
    )
)]
pub async fn cancel_build(
    State(store): State<Arc<Store>>,
    Path(build_id): Path<i32>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let build = store
        .builds()
        .find_by_id(build_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Build not found" })),
            )
        })?;

    if !matches!(build.status, BuildStatus::Queued | BuildStatus::Building) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Cannot cancel build in status {:?}", build.status)
            })),
        ));
    }

    let mut build_active = build.into_active_model();
    build_active.status = Set(BuildStatus::Cancelled);

    store.builds().update(build_active).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    Ok(StatusCode::OK)
}
