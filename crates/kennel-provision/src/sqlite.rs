use crate::{ResourceProvider, ResourceRequest};
use kennel_config::constants::{SQLITE_DB_FILE, SQLITE_STATE_DIR};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;

/// Per-unit SQLite file in a systemd `StateDirectory` named after the unit's `DynamicUser`
pub struct SqliteProvider;

pub fn state_directory(user: &str) -> String {
    format!("{SQLITE_STATE_DIR}/{user}")
}

pub fn database_path(user: &str) -> String {
    format!("/var/lib/{SQLITE_STATE_DIR}/{user}/{SQLITE_DB_FILE}")
}

impl SqliteProvider {
    /// Runs as root to reach `/var/lib/private`
    async fn remove(user: &str) -> anyhow::Result<()> {
        let private = format!("/var/lib/private/{SQLITE_STATE_DIR}/{user}");
        let link = format!("/var/lib/{SQLITE_STATE_DIR}/{user}");

        let out = tokio::process::Command::new("systemd-run")
            .args([
                "--quiet",
                "--wait",
                "--collect",
                "--no-ask-password",
                "--service-type=oneshot",
                "--description=Kennel: remove SQLite state",
                "--",
                "rm",
                "-rf",
                "--one-file-system",
                "--",
                &private,
                &link,
            ])
            .output()
            .await?;

        anyhow::ensure!(
            out.status.success(),
            "removing SQLite state for {user} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );

        tracing::info!(user = %user, "removed SQLite database");
        Ok(())
    }
}

impl ResourceProvider for SqliteProvider {
    fn name(&self) -> &str {
        "sqlite"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
        _config: &kennel_config::KennelConfig,
    ) -> anyhow::Result<HashMap<String, String>> {
        Ok(HashMap::from([(
            "DATABASE_PATH".into(),
            database_path(&request.system_user),
        )]))
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let user = &request.system_user;
        // static sites pass an empty user, which would wipe every database
        if user.is_empty() {
            return Ok(());
        }

        let link = format!("/var/lib/{SQLITE_STATE_DIR}/{user}");
        match tokio::fs::symlink_metadata(&link).await {
            Ok(_) => Self::remove(user).await,
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let root = format!("/var/lib/{SQLITE_STATE_DIR}");
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let active_users: HashSet<&str> = active.iter().map(|r| r.system_user.as_str()).collect();

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(user) = name.to_str() else {
                continue;
            };
            let plain_name = user.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            if active_users.contains(user) || !plain_name {
                continue;
            }

            tracing::info!(user = %user, "removing orphaned SQLite database");
            if let Err(e) = Self::remove(user).await {
                tracing::warn!(user = %user, error = %e, "failed to remove orphaned SQLite database");
            }
        }

        Ok(())
    }
}
