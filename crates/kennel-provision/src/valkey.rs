use crate::{ResourceProvider, ResourceRequest};
use std::collections::HashMap;

pub struct ValkeyProvider {
    socket_path: String,
}

impl ValkeyProvider {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    fn allocation_key(request: &ResourceRequest) -> String {
        format!(
            "{}/{}/{}",
            request.project_name, request.service_name, request.branch_slug
        )
    }

    /// Run `valkey-cli` against this instance's socket, returning trimmed stdout.
    async fn cli(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = tokio::process::Command::new("valkey-cli")
            .arg("-s")
            .arg(&self.socket_path)
            .args(args)
            .output()
            .await?;

        if !out.status.success() {
            anyhow::bail!(
                "valkey-cli {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }

        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Snapshot the DB, flush it, then remove its allocation entry.
    async fn deallocate(&self, db_num: u32, key: &str) -> anyhow::Result<()> {
        self.snapshot_before_flush(db_num, key).await?;

        let db = db_num.to_string();
        self.cli(&["-n", db.as_str(), "FLUSHDB"]).await?;
        self.cli(&["-n", "0", "HDEL", "kennel:allocations", key])
            .await?;

        tracing::info!(key = %key, db = db_num, "deallocated valkey DB");
        Ok(())
    }

    /// Stream a full dataset dump into the backup dir over the socket before flushing.
    async fn snapshot_before_flush(&self, db_num: u32, key: &str) -> anyhow::Result<()> {
        let backup_dir = std::path::Path::new(kennel_config::constants::VALKEY_BACKUP_DIR);
        std::fs::create_dir_all(backup_dir)?;

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let safe_key: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let dest = backup_dir.join(format!("predelete-{ts}-db{db_num}-{safe_key}.rdb"));
        let dest = dest
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("backup path is not valid UTF-8"))?;

        // --rdb streams a full dataset dump over the socket to a local file.
        self.cli(&["--rdb", dest]).await?;

        tracing::info!(db = db_num, key = %key, path = %dest, "snapshotted valkey before flush");
        Self::prune_backups(backup_dir);
        Ok(())
    }

    /// Keep only the most recent `VALKEY_BACKUP_KEEP` pre-delete snapshots.
    fn prune_backups(dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("predelete-"))
            })
            .collect();
        files.sort();

        let keep = kennel_config::constants::VALKEY_BACKUP_KEEP;
        if files.len() > keep {
            for path in &files[..files.len() - keep] {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl ResourceProvider for ValkeyProvider {
    fn name(&self) -> &str {
        "valkey"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let key = Self::allocation_key(request);

        // Check if already allocated by querying the allocation hash in DB 0
        let existing = tokio::process::Command::new("valkey-cli")
            .args([
                "-s",
                &self.socket_path,
                "-n",
                "0",
                "HGET",
                "kennel:allocations",
                &key,
            ])
            .output()
            .await?;

        let db_num = if existing.status.success() {
            let out = String::from_utf8_lossy(&existing.stdout).trim().to_string();
            if !out.is_empty() && out != "(nil)" {
                match out.parse::<u32>() {
                    Ok(n) => n,
                    Err(_) => {
                        tracing::error!(
                            key = %key,
                            value = %out,
                            "valkey allocation holds a non-numeric DB number; refusing to provision"
                        );
                        return Err(anyhow::anyhow!(
                            "valkey allocation for {key} is not a valid DB number"
                        ));
                    }
                }
            } else {
                // Find the next available DB number (1-31, DB 0 is reserved for metadata)
                let all = tokio::process::Command::new("valkey-cli")
                    .args([
                        "-s",
                        &self.socket_path,
                        "-n",
                        "0",
                        "HVALS",
                        "kennel:allocations",
                    ])
                    .output()
                    .await?;
                let used: std::collections::HashSet<u32> = String::from_utf8_lossy(&all.stdout)
                    .lines()
                    .filter_map(|l| l.parse().ok())
                    .collect();

                let next = (1..32u32)
                    .find(|n| !used.contains(n))
                    .ok_or_else(|| anyhow::anyhow!("no available Valkey DB numbers"))?;

                tokio::process::Command::new("valkey-cli")
                    .args([
                        "-s",
                        &self.socket_path,
                        "-n",
                        "0",
                        "HSET",
                        "kennel:allocations",
                        &key,
                        &next.to_string(),
                    ])
                    .output()
                    .await?;

                tracing::info!(key = %key, db = next, "allocated valkey DB");
                next
            }
        } else {
            return Err(anyhow::anyhow!("failed to query valkey allocations"));
        };

        let mut env = HashMap::new();
        env.insert(
            "VALKEY_URL".into(),
            format!("redis+unix://{}?db={db_num}", self.socket_path),
        );
        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let key = Self::allocation_key(request);

        let existing = self
            .cli(&["-n", "0", "HGET", "kennel:allocations", &key])
            .await?;
        if let Ok(db_num) = existing.parse::<u32>() {
            self.deallocate(db_num, &key).await?;
        }

        Ok(())
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let active_keys: std::collections::HashSet<String> =
            active.iter().map(Self::allocation_key).collect();

        let all = self
            .cli(&["-n", "0", "HKEYS", "kennel:allocations"])
            .await?;
        for key in all.lines() {
            if active_keys.contains(key) {
                continue;
            }

            let existing = self
                .cli(&["-n", "0", "HGET", "kennel:allocations", key])
                .await?;
            let Ok(db_num) = existing.parse::<u32>() else {
                tracing::warn!(key = %key, "orphaned allocation with unparseable DB number; leaving in place");
                continue;
            };

            tracing::info!(key = %key, db = db_num, "reconciling orphaned valkey allocation");
            if let Err(e) = self.deallocate(db_num, key).await {
                tracing::warn!(key = %key, error = %e, "failed to deallocate orphaned valkey DB");
            }
        }

        Ok(())
    }
}
