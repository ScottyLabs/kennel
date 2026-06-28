use crate::{ResourceProvider, ResourceRequest};
use std::collections::HashMap;

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
        format!("kennel-{}-{}", request.project_name, request.branch_slug)
    }

    fn key_name(request: &ResourceRequest) -> String {
        format!("kennel-{}-{}", request.project_name, request.branch_slug)
    }

    async fn api_get(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}{path}", self.admin_endpoint))
            .header("Authorization", format!("Bearer {}", self.admin_token))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn api_post(
        &self,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let mut req = self
            .client
            .post(format!("{}{path}", self.admin_endpoint))
            .header("Authorization", format!("Bearer {}", self.admin_token));

        if let Some(body) = body {
            req = req.json(body);
        }

        let text = req.send().await?.error_for_status()?.text().await?;
        if text.is_empty() {
            Ok(serde_json::Value::Null)
        } else {
            Ok(serde_json::from_str(&text)?)
        }
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
        let key_name = Self::key_name(request);

        // Find the bucket by global alias, or create it, capturing its id
        let buckets = self.api_get("/v2/ListBuckets").await?;
        let existing_bucket_id = buckets
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| {
                b["globalAliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|a| a.as_str() == Some(&bucket_name))
            })
            .and_then(|b| b["id"].as_str().map(String::from));

        let bucket_id = match existing_bucket_id {
            Some(id) => id,
            None => {
                let created = self
                    .api_post(
                        "/v2/CreateBucket",
                        Some(&serde_json::json!({ "globalAlias": bucket_name })),
                    )
                    .await?;
                tracing::info!(bucket = %bucket_name, "created garage bucket");
                created["id"].as_str().unwrap_or_default().to_string()
            }
        };

        // Find the key by name, or create it, capturing its id and secret
        // The secret is only returned at creation, so existing keys are re-read
        let keys = self.api_get("/v2/ListKeys").await?;
        let existing_key_id = keys
            .as_array()
            .into_iter()
            .flatten()
            .find(|k| k["name"].as_str() == Some(&key_name))
            .and_then(|k| k["id"].as_str().map(String::from));

        let (access_key_id, secret_access_key) = match existing_key_id {
            Some(id) => {
                let detail = self
                    .api_get(&format!("/v2/GetKeyInfo?id={id}&showSecretKey=true"))
                    .await?;
                (
                    id,
                    detail["secretAccessKey"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                )
            }
            None => {
                let key = self
                    .api_post(
                        "/v2/CreateKey",
                        Some(&serde_json::json!({ "name": key_name })),
                    )
                    .await?;
                tracing::info!(key = %key_name, "created garage API key");
                (
                    key["accessKeyId"].as_str().unwrap_or_default().to_string(),
                    key["secretAccessKey"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                )
            }
        };

        // Grant the key full access to the bucket, idempotent on re-provision
        self.api_post(
            "/v2/AllowBucketKey",
            Some(&serde_json::json!({
                "bucketId": bucket_id,
                "accessKeyId": access_key_id,
                "permissions": { "read": true, "write": true, "owner": true },
            })),
        )
        .await?;

        let mut env = HashMap::new();
        env.insert("S3_ENDPOINT".into(), self.s3_endpoint.clone());
        env.insert("S3_BUCKET".into(), bucket_name);
        env.insert("AWS_ACCESS_KEY_ID".into(), access_key_id);
        env.insert("AWS_SECRET_ACCESS_KEY".into(), secret_access_key);
        Ok(env)
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        let bucket_name = Self::bucket_name(request);
        let key_name = Self::key_name(request);

        // Delete the key by id
        let keys = self.api_get("/v2/ListKeys").await?;
        if let Some(key_id) = keys
            .as_array()
            .into_iter()
            .flatten()
            .find(|k| k["name"].as_str() == Some(&key_name))
            .and_then(|k| k["id"].as_str())
        {
            self.api_post(&format!("/v2/DeleteKey?id={key_id}"), None)
                .await?;
            tracing::info!(key = %key_name, "deleted garage API key");
        }

        // Delete the bucket by id, must be empty first
        let buckets = self.api_get("/v2/ListBuckets").await?;
        if let Some(bucket_id) = buckets
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| {
                b["globalAliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|a| a.as_str() == Some(&bucket_name))
            })
            .and_then(|b| b["id"].as_str())
        {
            self.api_post(&format!("/v2/DeleteBucket?id={bucket_id}"), None)
                .await?;
            tracing::info!(bucket = %bucket_name, "deleted garage bucket");
        }

        Ok(())
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let active_buckets: std::collections::HashSet<String> =
            active.iter().map(Self::bucket_name).collect();

        let buckets = self.api_get("/v2/ListBuckets").await?;
        for bucket in buckets.as_array().into_iter().flatten() {
            for alias in bucket["globalAliases"].as_array().into_iter().flatten() {
                if let Some(name) = alias.as_str()
                    && name.starts_with("kennel-")
                    && !active_buckets.contains(name)
                {
                    tracing::info!(bucket = %name, "deleting orphaned garage bucket");
                    if let Some(id) = bucket["id"].as_str() {
                        let _ = self
                            .api_post(&format!("/v2/DeleteBucket?id={id}"), None)
                            .await;
                    }
                }
            }
        }

        Ok(())
    }
}
