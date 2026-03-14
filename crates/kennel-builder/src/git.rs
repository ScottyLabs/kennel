use crate::error::{BuilderError, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

pub async fn clone(repo_url: &str, git_ref: &str, commit_sha: &str, work_dir: &Path) -> Result<()> {
    info!(
        "Cloning repository {} (ref: {}, sha: {})",
        repo_url, git_ref, commit_sha
    );

    tokio::fs::create_dir_all(work_dir).await?;

    let repo_path = work_dir.join("repo");

    let output = Command::new("git")
        .arg("init")
        .arg("repo")
        .current_dir(work_dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!("git init failed: {stderr}")));
    }

    let output = Command::new("git")
        .arg("remote")
        .arg("add")
        .arg("origin")
        .arg(repo_url)
        .current_dir(&repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!(
            "git remote add failed: {stderr}"
        )));
    }

    let output = Command::new("git")
        .arg("fetch")
        .arg("--depth")
        .arg("1")
        .arg("origin")
        .arg("--")
        .arg(git_ref)
        .current_dir(&repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!("git fetch failed: {stderr}")));
    }

    debug!("Fetch successful, checking out FETCH_HEAD");

    let output = Command::new("git")
        .arg("checkout")
        .arg("FETCH_HEAD")
        .current_dir(&repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::Git(format!("git checkout failed: {stderr}")));
    }

    // Verify the checked-out commit matches the expected SHA.
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&repo_path)
        .output()
        .await?;

    let head_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !head_sha.starts_with(commit_sha) && !commit_sha.starts_with(&head_sha) {
        return Err(BuilderError::Git(format!(
            "SHA mismatch: expected {commit_sha}, got {head_sha}"
        )));
    }

    info!("Repository cloned and checked out at {head_sha}");
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
            "refs/heads/main",
            "abc123",
            &work_dir,
        )
        .await;

        assert!(result.is_err());
    }
}
