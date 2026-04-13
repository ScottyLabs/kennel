use crate::AppState;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run_once(_state: &AppState) -> anyhow::Result<()> {
    // TODO: reconcile deployments against systemd + caddy
    Ok(())
}

pub async fn run_loop(state: Arc<AppState>, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(kennel_config::constants::RECONCILE_INTERVAL);

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = state.signal.notified() => {}
            _ = cancel.cancelled() => break,
        }

        if let Err(e) = run_once(&state).await {
            tracing::error!(error = %e, "reconciliation failed");
        }
    }
}
