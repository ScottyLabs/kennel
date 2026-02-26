use crate::error::{BuilderError, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

pub async fn clone(repo_url: &str, commit_sha: &str, work_dir: &Path) -> Result<()> {
    info!("Cloning repository {} at commit {}", repo_url, commit_sha);

    // Create work directory
    tokio::fs::create_dir_all(work_dir).await?;

    // Clone with depth 1
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(repo_url)
        .arg("repo")
        .current_dir(work_dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!("Clone failed: {}", stderr)));
    }

    debug!("Clone successful, checking out commit");

    let repo_path = work_dir.join("repo");

    // Fetch the specific commit
    let output = Command::new("git")
        .arg("fetch")
        .arg("origin")
        .arg(commit_sha)
        .current_dir(&repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!(
            "Fetch commit failed: {}",
            stderr
        )));
    }

    // Checkout the commit
    let output = Command::new("git")
        .arg("checkout")
        .arg(commit_sha)
        .current_dir(&repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!("Checkout failed: {}", stderr)));
    }

    info!("Repository cloned and checked out successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_clone_invalid_repo() {
        let temp_dir = TempDir::new().unwrap();
        let work_dir = temp_dir.path().join("build");

        let result = clone(
            "https://invalid-repo-url-that-does-not-exist.com/repo.git",
            "abc123",
            &work_dir,
        )
        .await;

        assert!(result.is_err());
    }
}
