use crate::error::Result;
use std::path::Path;
use tokio::process::Command;
use tracing::{error, info, warn};

pub fn generate_service_unit(
    service_name: &str,
    store_path: &str,
    port: u16,
    user: &str,
    working_dir: &Path,
    env_vars: &[(String, String)],
    secrets_path: Option<&Path>,
) -> String {
    let mut unit = format!(
        r#"[Unit]
Description=Kennel service: {service_name}
After=network.target

[Service]
Type=simple
User={user}
WorkingDirectory={working_dir}
ExecStart={store_path}/bin/{service_name}
Restart=on-failure
RestartSec=5s

Environment="PORT={port}"
"#,
        working_dir = working_dir.display()
    );

    for (key, value) in env_vars {
        unit.push_str(&format!("Environment=\"{}={}\"\n", key, value));
    }

    if let Some(secrets) = secrets_path {
        unit.push_str(&format!("EnvironmentFile={}\n", secrets.display()));
    }

    unit.push_str("\n[Install]\nWantedBy=multi-user.target\n");
    unit
}

pub async fn install_unit(unit_name: &str, unit_content: &str) -> Result<()> {
    let unit_path = format!(
        "{}/{}.service",
        kennel_config::constants::SYSTEMD_UNIT_DIR,
        unit_name
    );

    tokio::fs::write(&unit_path, unit_content).await?;
    info!("Installed systemd unit: {}", unit_name);

    Ok(())
}

pub async fn daemon_reload() -> Result<()> {
    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("systemctl daemon-reload failed: {}", stderr);
        return Err(crate::DeployerError::Systemd(format!(
            "daemon-reload failed: {}",
            stderr
        )));
    }

    Ok(())
}

pub async fn enable_unit(unit_name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("enable")
        .arg(format!("{}.service", unit_name))
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("systemctl enable failed for {}: {}", unit_name, stderr);
        return Err(crate::DeployerError::Systemd(format!(
            "enable failed: {}",
            stderr
        )));
    }

    info!("Enabled systemd unit: {}", unit_name);
    Ok(())
}

pub async fn start_unit(unit_name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("start")
        .arg(format!("{}.service", unit_name))
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("systemctl start failed for {}: {}", unit_name, stderr);
        return Err(crate::DeployerError::Systemd(format!(
            "start failed: {}",
            stderr
        )));
    }

    info!("Started systemd unit: {}", unit_name);
    Ok(())
}

pub async fn stop_unit(unit_name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("stop")
        .arg(format!("{}.service", unit_name))
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("systemctl stop failed for {}: {}", unit_name, stderr);
    } else {
        info!("Stopped systemd unit: {}", unit_name);
    }

    Ok(())
}

pub async fn disable_unit(unit_name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("disable")
        .arg(format!("{}.service", unit_name))
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("systemctl disable failed for {}: {}", unit_name, stderr);
    } else {
        info!("Disabled systemd unit: {}", unit_name);
    }

    Ok(())
}

pub async fn remove_unit(unit_name: &str) -> Result<()> {
    let unit_path = format!(
        "{}/{}.service",
        kennel_config::constants::SYSTEMD_UNIT_DIR,
        unit_name
    );

    if let Err(e) = tokio::fs::remove_file(&unit_path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e.into());
    }

    info!("Removed systemd unit file: {}", unit_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_service_unit() {
        let unit = generate_service_unit(
            "test-api",
            "/nix/store/abc123-test-api",
            8080,
            "kennel-test-api",
            Path::new("/var/lib/kennel/services/test-project/main/test-api"),
            &[(
                "DATABASE_URL".to_string(),
                "postgres://localhost/test".to_string(),
            )],
            Some(Path::new("/run/kennel/secrets/test-api.env")),
        );

        assert!(unit.contains("Description=Kennel service: test-api"));
        assert!(unit.contains("User=kennel-test-api"));
        assert!(unit.contains("ExecStart=/nix/store/abc123-test-api/bin/test-api"));
        assert!(unit.contains("Environment=\"PORT=8080\""));
        assert!(unit.contains("Environment=\"DATABASE_URL=postgres://localhost/test\""));
        assert!(unit.contains("EnvironmentFile=/run/kennel/secrets/test-api.env"));
    }
}
