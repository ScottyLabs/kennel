use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

use crate::AppState;
use crate::handlers::{deployments, metrics, webhook};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/webhook", post(webhook::handle))
        .route("/metrics", get(metrics::scrape))
        .route("/deployments/{id}/health", get(deployments::health))
        .with_state(state)
}
