use crate::error::Result;
use tokio::process::Command;
use tracing::{info, warn};

pub async fn ensure_user_exists(username: &str) -> Result<()> {
    let output = Command::new("id").arg(username).output().await?;

    if output.status.success() {
        info!("User {} already exists", username);
        return Ok(());
    }

    let output = Command::new("useradd")
        .arg("--system")
        .arg("--no-create-home")
        .arg("--shell")
        .arg("/bin/false")
        .arg(username)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::DeployerError::Other(anyhow::anyhow!(
            "Failed to create user {}: {}",
            username,
            stderr
        )));
    }

    info!("Created system user: {}", username);
    Ok(())
}

pub async fn remove_user(username: &str) -> Result<()> {
    let output = Command::new("userdel").arg(username).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("Failed to remove user {}: {}", username, stderr);
    } else {
        info!("Removed system user: {}", username);
    }

    Ok(())
}
