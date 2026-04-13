use crate::AppState;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run_worker(_state: Arc<AppState>, cancel: CancellationToken) {
    // TODO: claim queued builds, clone, nix build, mark built
    cancel.cancelled().await;
}
