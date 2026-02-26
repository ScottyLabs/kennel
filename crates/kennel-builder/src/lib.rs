mod cachix;
mod error;
mod git;
mod nix;
mod worker;

pub use error::{BuilderError, Result};

use kennel_store::Store;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tracing::{error, info};

#[derive(Clone)]
pub struct BuilderConfig {
    pub store: Arc<Store>,
    pub deploy_tx: mpsc::Sender<DeploymentRequest>,
    pub max_concurrent_builds: usize,
    pub work_dir: String,
}

#[derive(Debug, Clone)]
pub struct DeploymentRequest {
    pub build_id: i64,
    pub project_name: String,
    pub git_ref: String,
}

pub async fn run_worker_pool(mut build_rx: mpsc::Receiver<i64>, config: BuilderConfig) {
    info!(
        "Starting builder worker pool with max_concurrent_builds={}",
        config.max_concurrent_builds
    );

    let semaphore = Arc::new(Semaphore::new(config.max_concurrent_builds));
    let config = Arc::new(config);

    while let Some(build_id) = build_rx.recv().await {
        info!("Received build request for build {}", build_id);

        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let config = config.clone();

        tokio::spawn(async move {
            if let Err(e) = worker::process_build(build_id, config).await {
                error!("Build {} failed: {}", build_id, e);
            }
            drop(permit);
        });
    }

    info!("Builder worker pool shutting down");
}
