mod error;
mod events;
mod handler;
mod parse;
mod verify;

pub use error::{Result, WebhookError};
pub use events::WebhookEvent;

use axum::{Router, routing::post};
use kennel_store::Store;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct WebhookConfig {
    pub store: Arc<Store>,
    pub build_tx: mpsc::Sender<i32>,
}

pub fn router(config: WebhookConfig) -> Router {
    Router::new()
        .route("/webhook/{project}", post(handler::handle_webhook))
        .with_state(Arc::new(config))
}
