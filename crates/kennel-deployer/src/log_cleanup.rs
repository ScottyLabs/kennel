use crate::DeployerConfig;
use kennel_config::constants;
use std::path::PathBuf;
use tracing::{error, info, warn};

async fn clean_build(config: &DeployerConfig, build_id: i32) {
    let log_dir = PathBuf::from(constants::LOGS_DIR).join(build_id.to_string());

    if log_dir.exists()
        && let Err(e) = tokio::fs::remove_dir_all(&log_dir).await
    {
        warn!("Failed to remove log directory {:?}: {}", log_dir, e);
    }

    if let Err(e) = config.store.builds().delete(build_id).await {
        error!("Failed to delete build record {}: {}", build_id, e);
    }
}

/// Periodically deletes build logs and records older than the retention period.
pub async fn run_log_cleanup_job(config: DeployerConfig) {
    info!("Starting build log cleanup job");

    let mut interval = tokio::time::interval(constants::LOG_CLEANUP_INTERVAL);

    loop {
        interval.tick().await;

        info!("Running build log cleanup");

        match config
            .store
            .find_old_builds(constants::LOG_RETENTION_DAYS)
            .await
        {
            Ok(old_builds) if !old_builds.is_empty() => {
                for build in &old_builds {
                    clean_build(&config, build.id).await;
                }
                info!(
                    "Cleaned up {} old build(s) and their logs",
                    old_builds.len()
                );
            }
            Err(e) => {
                error!("Log cleanup failed to find old builds: {}", e);
            }
            _ => {}
        }
    }
}
