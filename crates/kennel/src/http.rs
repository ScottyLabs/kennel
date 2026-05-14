use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

use crate::AppState;
use crate::handlers::{caddy, webhook};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/webhook", post(webhook::handle))
        .route("/internal/caddy/check-domain", get(caddy::check_domain))
        .with_state(state)
}
