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
        format!("{}/{}", request.project_name, request.branch_slug)
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
            .args(["-s", &self.socket_path, "-n", "0", "HGET", "kennel:allocations", &key])
            .output()
            .await?;

        let db_num = if existing.status.success() {
            let out = String::from_utf8_lossy(&existing.stdout).trim().to_string();
            if !out.is_empty() && out != "(nil)" {
                out.parse::<u32>().unwrap_or(0)
            } else {
                // Find the next available DB number (1-31, DB 0 is reserved for metadata)
                let all = tokio::process::Command::new("valkey-cli")
                    .args(["-s", &self.socket_path, "-n", "0", "HVALS", "kennel:allocations"])
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
                        "-s", &self.socket_path, "-n", "0",
                        "HSET", "kennel:allocations", &key, &next.to_string(),
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

        let existing = tokio::process::Command::new("valkey-cli")
            .args(["-s", &self.socket_path, "-n", "0", "HGET", "kennel:allocations", &key])
            .output()
            .await?;

        let out = String::from_utf8_lossy(&existing.stdout).trim().to_string();
        if let Ok(db_num) = out.parse::<u32>() {
            // Flush the allocated DB
            tokio::process::Command::new("valkey-cli")
                .args(["-s", &self.socket_path, "-n", &db_num.to_string(), "FLUSHDB"])
                .output()
                .await?;

            // Remove the allocation
            tokio::process::Command::new("valkey-cli")
                .args(["-s", &self.socket_path, "-n", "0", "HDEL", "kennel:allocations", &key])
                .output()
                .await?;

            tracing::info!(key = %key, db = db_num, "deallocated valkey DB");
        }

        Ok(())
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let active_keys: std::collections::HashSet<String> =
            active.iter().map(Self::allocation_key).collect();

        let all = tokio::process::Command::new("valkey-cli")
            .args(["-s", &self.socket_path, "-n", "0", "HKEYS", "kennel:allocations"])
            .output()
            .await?;

        for key in String::from_utf8_lossy(&all.stdout).lines() {
            if !active_keys.contains(key) {
                tracing::info!(key = %key, "removing orphaned valkey allocation");
                let _ = tokio::process::Command::new("valkey-cli")
                    .args(["-s", &self.socket_path, "-n", "0", "HDEL", "kennel:allocations", key])
                    .output()
                    .await;
            }
        }

        Ok(())
    }
}
