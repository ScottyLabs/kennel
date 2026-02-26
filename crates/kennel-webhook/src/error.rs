use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WebhookError {
    #[error("signature verification failed")]
    InvalidSignature,

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("invalid webhook payload: {0}")]
    InvalidPayload(String),

    #[error("missing required header: {0}")]
    MissingHeader(&'static str),

    #[error("builder unavailable")]
    BuilderUnavailable,

    #[error(transparent)]
    Store(#[from] kennel_store::StoreError),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, WebhookError>;

impl IntoResponse for WebhookError {
    fn into_response(self) -> Response {
        let status = match &self {
            WebhookError::InvalidSignature => StatusCode::UNAUTHORIZED,
            WebhookError::ProjectNotFound(_) => StatusCode::NOT_FOUND,
            WebhookError::InvalidPayload(_) => StatusCode::BAD_REQUEST,
            WebhookError::MissingHeader(_) => StatusCode::BAD_REQUEST,
            WebhookError::BuilderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            WebhookError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
            WebhookError::Json(_) => StatusCode::BAD_REQUEST,
        };

        (status, self.to_string()).into_response()
    }
}
