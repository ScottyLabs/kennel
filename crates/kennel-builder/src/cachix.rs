use crate::error::Result;
use tokio::process::Command;
use tracing::{error, info};

pub async fn push_to_cachix(cache_name: &str, store_paths: &[String]) -> Result<()> {
    if store_paths.is_empty() {
        return Ok(());
    }

    info!(
        "Pushing {} store paths to Cachix cache '{}'",
        store_paths.len(),
        cache_name
    );

    let mut cmd = Command::new("cachix");
    cmd.arg("push").arg(cache_name);

    for path in store_paths {
        cmd.arg(path);
    }

    let output = cmd.output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Cachix push failed: {}", stderr);
        return Err(crate::BuilderError::Other(anyhow::anyhow!(
            "Cachix push failed: {}",
            stderr
        )));
    }

    info!("Successfully pushed to Cachix");
    Ok(())
}
