use crate::{ResourceProvider, ResourceRequest};
use std::collections::HashMap;

pub struct GarageProvider {
    admin_endpoint: String,
    s3_endpoint: String,
    admin_token: String,
}

impl GarageProvider {
    pub fn new(admin_endpoint: String, s3_endpoint: String, admin_token: String) -> Self {
        Self {
            admin_endpoint,
            s3_endpoint,
            admin_token,
        }
    }

    fn bucket_name(request: &ResourceRequest) -> String {
        format!("kennel-{}-{}", request.project_name, request.branch_slug)
    }
}

impl ResourceProvider for GarageProvider {
    fn name(&self) -> &str {
        "garage"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let bucket_name = Self::bucket_name(request);
        // TODO: create bucket + API key via Garage admin API (idempotent)
        let mut env = HashMap::new();
        env.insert("S3_ENDPOINT".into(), self.s3_endpoint.clone());
        env.insert("S3_BUCKET".into(), bucket_name);
        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let _bucket_name = Self::bucket_name(request);
        // TODO: empty bucket, delete bucket, delete API key
        Ok(())
    }

    async fn reconcile(&self, _active: &[ResourceRequest]) -> anyhow::Result<()> {
        // TODO: list kennel-* buckets, delete orphans
        Ok(())
    }
}
