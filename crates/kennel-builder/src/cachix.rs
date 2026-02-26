use crate::error::Result;
use kennel_config::CachixConfig;
use tokio::process::Command;
use tracing::{error, info};

pub async fn push_to_cachix(config: &CachixConfig, store_paths: &[String]) -> Result<()> {
    if store_paths.is_empty() {
        return Ok(());
    }

    info!(
        "Pushing {} store paths to Cachix cache '{}'",
        store_paths.len(),
        config.cache_name
    );

    let mut cmd = Command::new("cachix");
    cmd.arg("push").arg(&config.cache_name);

    if let Some(auth_token) = &config.auth_token {
        cmd.env("CACHIX_AUTH_TOKEN", auth_token);
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_push_empty_paths() {
        let config = CachixConfig {
            cache_name: "test-cache".to_string(),
            auth_token: None,
        };

        let result = push_to_cachix(&config, &[]).await;
        assert!(result.is_ok());
    }
}
