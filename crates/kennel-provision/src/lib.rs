pub mod garage;
pub mod postgres;
pub mod valkey;

use std::collections::HashMap;

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ResourceRequest {
    pub deployment_id: i32,
    pub project_name: String,
    pub service_name: String,
    pub branch: String,
    pub branch_slug: String,
    pub environment: String,
    pub system_user: String,
}

#[derive(Debug, Default)]
pub struct ReconciliationSummary {
    pub orphaned_resources_removed: usize,
}

#[async_trait]
pub trait ResourceProvider: Send + Sync {
    fn name(&self) -> &str;

    async fn provision(&self, request: &ResourceRequest)
    -> anyhow::Result<HashMap<String, String>>;

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()>;

    async fn reconcile(
        &self,
        active_deployments: &[ResourceRequest],
    ) -> anyhow::Result<ReconciliationSummary>;
}
