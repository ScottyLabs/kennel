use crate::error::Result;
use std::path::{Path, PathBuf};
use tracing::info;

pub async fn generate_env_file(
    project: &str,
    branch: &str,
    service: &str,
    env_vars: &[(String, String)],
) -> Result<PathBuf> {
    let secrets_dir = Path::new(kennel_config::constants::SECRETS_DIR);
    tokio::fs::create_dir_all(&secrets_dir).await?;

    let filename = format!("{}-{}-{}.env", project, branch, service);
    let secrets_path = secrets_dir.join(&filename);

    let mut content = String::new();
    for (key, value) in env_vars {
        content.push_str(&format!("{}={}\n", key, value));
    }

    tokio::fs::write(&secrets_path, content).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&secrets_path).await?.permissions();
        perms.set_mode(0o400);
        tokio::fs::set_permissions(&secrets_path, perms).await?;
    }

    info!("Generated env file: {}", secrets_path.display());
    Ok(secrets_path)
}

pub async fn remove_secrets_file(secrets_path: &Path) -> Result<()> {
    if let Err(e) = tokio::fs::remove_file(secrets_path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e.into());
    }

    info!("Removed secrets file: {}", secrets_path.display());
    Ok(())
}
