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

pub fn sanitize_username(project: &str, branch: &str, service: &str) -> String {
    let combined = format!("kennel-{}-{}-{}", project, branch, service);
    combined
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_username() {
        assert_eq!(
            sanitize_username("my-project", "main", "api"),
            "kennel-my-project-main-api"
        );
        assert_eq!(
            sanitize_username("Project_1", "feature/new", "web"),
            "kennel-project-1-feature-new-web"
        );
    }
}
