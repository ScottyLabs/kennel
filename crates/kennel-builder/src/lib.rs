mod cachix;
mod error;
mod git;
mod nix;
mod worker;

pub use error::{BuilderError, Result};

use kennel_store::Store;
use std::sync::Arc;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Clone)]
pub struct BuilderConfig {
    pub store: Arc<Store>,
    pub build_signal: Arc<Notify>,
    pub deploy_signal: Arc<Notify>,
    pub cancel: CancellationToken,
    pub max_concurrent_builds: usize,
    pub work_dir: String,
}

pub async fn run_worker_pool(config: BuilderConfig) {
    info!(
        "Starting builder worker pool with max_concurrent_builds={}",
        config.max_concurrent_builds
    );

    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_builds));
    let config = Arc::new(config);

    loop {
        if config.cancel.is_cancelled() {
            break;
        }

        // Try to claim a queued build from the database.
        let build = match config.store.builds().claim_queued_build().await {
            Ok(Some(build)) => build,
            Ok(None) => {
                // No work available, wait for a signal or cancellation.
                tokio::select! {
                    _ = config.build_signal.notified() => continue,
                    _ = config.cancel.cancelled() => break,
                }
            }
            Err(e) => {
                error!("Failed to claim build: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let build_id = build.id;
        info!("Claimed build {build_id} for processing");

        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let config = config.clone();

        tokio::spawn(async move {
            match worker::process_build(build_id, config.clone()).await {
                Ok(()) => {
                    // Build succeeded and was marked Built -- wake the deployer.
                    config.deploy_signal.notify_one();
                }
                Err(e) => {
                    error!("Build {build_id} failed: {e}");
                }
            }
            drop(permit);
        });
    }

    info!("Builder worker pool shutting down");
}
