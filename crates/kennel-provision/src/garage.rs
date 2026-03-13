use std::collections::HashMap;

use async_trait::async_trait;

use crate::{ReconciliationSummary, ResourceProvider, ResourceRequest};

pub struct GarageProvider {
    admin_endpoint: String,
    s3_endpoint: String,
    admin_token: String,
    client: reqwest::Client,
}

impl GarageProvider {
    pub fn new(admin_endpoint: String, s3_endpoint: String, admin_token: String) -> Self {
        Self {
            admin_endpoint,
            s3_endpoint,
            admin_token,
            client: reqwest::Client::new(),
        }
    }

    fn bucket_name(request: &ResourceRequest) -> String {
        format!(
            "kennel-{}-{}-{}",
            request.project_name, request.branch_slug, request.service_name
        )
    }

    async fn create_bucket(&self, bucket_name: &str) -> anyhow::Result<String> {
        let response = self
            .client
            .post(format!("{}/v2/CreateBucket", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .json(&serde_json::json!({
                "globalAlias": bucket_name
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to create bucket {bucket_name}: {body}");
        }

        let body: serde_json::Value = response.json().await?;
        let bucket_id = body["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No bucket ID in response"))?
            .to_string();

        Ok(bucket_id)
    }

    async fn create_key(&self, key_name: &str) -> anyhow::Result<(String, String)> {
        let response = self
            .client
            .post(format!("{}/v2/CreateKey", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .json(&serde_json::json!({
                "name": key_name
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to create key {key_name}: {body}");
        }

        let body: serde_json::Value = response.json().await?;
        let access_key_id = body["accessKeyId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No accessKeyId in response"))?
            .to_string();
        let secret_access_key = body["secretAccessKey"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No secretAccessKey in response"))?
            .to_string();

        Ok((access_key_id, secret_access_key))
    }

    async fn allow_bucket_key(&self, bucket_id: &str, access_key_id: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!("{}/v2/AllowBucketKey", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .json(&serde_json::json!({
                "bucketId": bucket_id,
                "accessKeyId": access_key_id,
                "permissions": {
                    "read": true,
                    "write": true,
                    "owner": false
                }
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to grant bucket access: {body}");
        }

        Ok(())
    }

    async fn delete_bucket(&self, bucket_id: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!(
                "{}/v2/DeleteBucket?id={bucket_id}",
                self.admin_endpoint
            ))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to delete bucket {bucket_id}: {body}");
        }

        Ok(())
    }

    async fn delete_key(&self, access_key_id: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!(
                "{}/v2/DeleteKey?id={access_key_id}",
                self.admin_endpoint
            ))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to delete key {access_key_id}: {body}");
        }

        Ok(())
    }
}

#[async_trait]
impl ResourceProvider for GarageProvider {
    fn name(&self) -> &str {
        "garage"
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        let bucket_name = Self::bucket_name(request);
        let key_name = format!("kennel-{}-{}", request.project_name, request.branch_slug);

        tracing::info!("Creating Garage bucket: {bucket_name}");
        let bucket_id = self.create_bucket(&bucket_name).await?;

        tracing::info!("Creating Garage API key: {key_name}");
        let (access_key_id, secret_access_key) = self.create_key(&key_name).await?;

        tracing::info!("Granting key access to bucket");
        self.allow_bucket_key(&bucket_id, &access_key_id).await?;

        let mut env = HashMap::new();
        env.insert("S3_ENDPOINT".into(), self.s3_endpoint.clone());
        env.insert("S3_BUCKET".into(), bucket_name);
        env.insert("AWS_ACCESS_KEY_ID".into(), access_key_id);
        env.insert("AWS_SECRET_ACCESS_KEY".into(), secret_access_key);

        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let bucket_name = Self::bucket_name(request);
        let key_name = format!("kennel-{}-{}", request.project_name, request.branch_slug);

        tracing::info!("Deleting Garage bucket and key: {bucket_name}");

        // Delete bucket.
        let response = self
            .client
            .get(format!("{}/v2/ListBuckets", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if response.status().is_success() {
            let buckets: Vec<serde_json::Value> = response.json().await?;
            for bucket in &buckets {
                let aliases = bucket["globalAliases"].as_array();
                if aliases.is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&bucket_name)))
                    && let Some(bucket_id) = bucket["id"].as_str()
                    && let Err(e) = self.delete_bucket(bucket_id).await
                {
                    tracing::warn!("Failed to delete bucket {bucket_name}: {e}");
                }
            }
        }

        // Delete associated API key.
        let response = self
            .client
            .get(format!("{}/v2/ListKeys", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if response.status().is_success() {
            let keys: Vec<serde_json::Value> = response.json().await?;
            for key in &keys {
                if key["name"].as_str() == Some(&key_name)
                    && let Some(key_id) = key["accessKeyId"].as_str()
                    && let Err(e) = self.delete_key(key_id).await
                {
                    tracing::warn!("Failed to delete key {key_name}: {e}");
                }
            }
        }

        Ok(())
    }

    async fn reconcile(
        &self,
        active_deployments: &[ResourceRequest],
    ) -> anyhow::Result<ReconciliationSummary> {
        let active_bucket_names: std::collections::HashSet<String> =
            active_deployments.iter().map(Self::bucket_name).collect();

        let response = self
            .client
            .get(format!("{}/v2/ListBuckets", self.admin_endpoint))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to list buckets for reconciliation");
        }

        let buckets: Vec<serde_json::Value> = response.json().await?;
        let mut summary = ReconciliationSummary::default();

        for bucket in &buckets {
            let aliases = bucket["globalAliases"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            for alias in aliases {
                if alias.starts_with("kennel-")
                    && !active_bucket_names.contains(alias)
                    && let Some(bucket_id) = bucket["id"].as_str()
                {
                    tracing::info!("Removing orphaned Garage bucket: {alias}");
                    let _ = self.delete_bucket(bucket_id).await;
                    summary.orphaned_resources_removed += 1;
                }
            }
        }

        Ok(summary)
    }
}
