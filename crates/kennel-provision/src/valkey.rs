use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::{ReconciliationSummary, ResourceProvider, ResourceRequest};

const MAX_DBS: u32 = 32;

pub struct ValkeyProvider {
    socket_path: String,
    allocated_dbs: Mutex<HashMap<String, u32>>,
}

impl ValkeyProvider {
    pub fn new(socket_path: String) -> Self {
        Self {
            socket_path,
            allocated_dbs: Mutex::new(HashMap::new()),
        }
    }

    fn allocation_key(request: &ResourceRequest) -> String {
        format!("{}/{}", request.project_name, request.branch_slug)
    }
}

#[async_trait]
impl ResourceProvider for ValkeyProvider {
    fn name(&self) -> &str {
        "valkey"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let key = Self::allocation_key(request);

        let db_num = {
            let mut allocated = self.allocated_dbs.lock().unwrap();
            if let Some(&existing) = allocated.get(&key) {
                existing
            } else {
                let used: std::collections::HashSet<u32> = allocated.values().copied().collect();
                let db = (0..MAX_DBS)
                    .find(|db| !used.contains(db))
                    .ok_or_else(|| anyhow::anyhow!("Valkey DB pool exhausted (max {MAX_DBS})"))?;
                allocated.insert(key, db);
                db
            }
        };

        tracing::info!(
            "Allocated Valkey DB {db_num} for {}/{}",
            request.project_name,
            request.branch_slug
        );

        let mut env = HashMap::new();
        env.insert(
            "VALKEY_URL".into(),
            format!("redis+unix:///{}?db={db_num}", self.socket_path),
        );

        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let key = Self::allocation_key(request);

        let db_num = {
            let mut allocated = self.allocated_dbs.lock().unwrap();
            allocated.remove(&key)
        };

        if let Some(db_num) = db_num {
            tracing::info!(
                "Released Valkey DB {db_num} for {}/{}",
                request.project_name,
                request.branch_slug
            );
        }

        Ok(())
    }

    async fn reconcile(
        &self,
        active_deployments: &[ResourceRequest],
    ) -> anyhow::Result<ReconciliationSummary> {
        let active_keys: std::collections::HashSet<String> = active_deployments
            .iter()
            .map(Self::allocation_key)
            .collect();

        let mut summary = ReconciliationSummary::default();

        let mut allocated = self.allocated_dbs.lock().unwrap();
        let orphaned: Vec<String> = allocated
            .keys()
            .filter(|k| !active_keys.contains(*k))
            .cloned()
            .collect();

        for key in orphaned {
            if let Some(db_num) = allocated.remove(&key) {
                tracing::info!("Released orphaned Valkey DB {db_num} for {key}");
                summary.orphaned_resources_removed += 1;
            }
        }

        Ok(summary)
    }
}
