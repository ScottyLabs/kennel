use crate::error::{BuilderError, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

pub async fn build(work_dir: &Path, service_name: &str, build_id: i64) -> Result<String> {
    info!("Building Nix package for service: {}", service_name);

    let repo_path = work_dir.join("repo");
    let out_link = work_dir.join(service_name);
    let log_file = work_dir
        .parent()
        .unwrap()
        .join("logs")
        .join(build_id.to_string())
        .join(format!("{}.log", service_name));

    // Create log directory
    if let Some(parent) = log_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Build the Nix package
    let flake_ref = format!(".#packages.x86_64-linux.{}", service_name);

    debug!("Running nix build {}", flake_ref);

    let output = Command::new("nix")
        .arg("build")
        .arg(&flake_ref)
        .arg("--out-link")
        .arg(&out_link)
        .arg("--log-format")
        .arg("bar-with-logs")
        .current_dir(&repo_path)
        .output()
        .await?;

    // Write stderr to log file
    tokio::fs::write(&log_file, &output.stderr).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuilderError::NixBuild(format!(
            "Build failed for {}: {}",
            service_name, stderr
        )));
    }

    // Read the store path from the symlink
    let store_path = tokio::fs::read_link(&out_link).await?;
    let store_path_str = store_path
        .to_str()
        .ok_or_else(|| BuilderError::InvalidStorePath(format!("{:?}", store_path)))?
        .to_string();

    info!(
        "Build successful for {}, store path: {}",
        service_name, store_path_str
    );

    Ok(store_path_str)
}

pub fn validate_service_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(BuilderError::NixBuild(
            "Service name cannot be empty".to_string(),
        ));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(BuilderError::NixBuild(format!(
            "Invalid service name '{}': must contain only lowercase letters, digits, and hyphens",
            name
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_service_name_valid() {
        assert!(validate_service_name("my-service").is_ok());
        assert!(validate_service_name("api").is_ok());
        assert!(validate_service_name("web-123").is_ok());
    }

    #[test]
    fn test_validate_service_name_invalid() {
        assert!(validate_service_name("").is_err());
        assert!(validate_service_name("My-Service").is_err());
        assert!(validate_service_name("my_service").is_err());
        assert!(validate_service_name("my.service").is_err());
    }
}
