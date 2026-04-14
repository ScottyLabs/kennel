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
            .await?;
        Ok(resp.json().await?)
    }

    async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .client
            .post(format!("{}{path}", self.admin_endpoint))
            .header("Authorization", format!("Bearer {}", self.admin_token))
            .json(body)
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    async fn api_delete(&self, path: &str) -> anyhow::Result<()> {
        self.client
            .delete(format!("{}{path}", self.admin_endpoint))
            .header("Authorization", format!("Bearer {}", self.admin_token))
            .send()
            .await?;
        Ok(())
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

        // Check if bucket exists
        let buckets = self.api_get("/v1/bucket?list").await?;
        let bucket_exists = buckets.as_array().unwrap_or(&vec![]).iter().any(|b| {
            b["globalAliases"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .any(|a| a.as_str() == Some(&bucket_name))
        });

        if !bucket_exists {
            self.api_post(
                "/v1/bucket",
                &serde_json::json!({"globalAlias": bucket_name}),
            )
            .await?;
            tracing::info!(bucket = %bucket_name, "created garage bucket");
        }

        // Check if key exists
        let keys = self.api_get("/v1/key?list").await?;
        let existing_key = keys
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .find(|k| k["name"].as_str() == Some(&key_name))
            .cloned();

        let (access_key_id, secret_access_key) = if let Some(key) = existing_key {
            let key_id = key["accessKeyId"].as_str().unwrap_or("").to_string();
            let detail = self.api_get(&format!("/v1/key?id={key_id}")).await?;
            (
                key_id,
                detail["secretAccessKey"].as_str().unwrap_or("").to_string(),
            )
        } else {
            let key = self
                .api_post("/v1/key", &serde_json::json!({"name": key_name}))
                .await?;
            let key_id = key["accessKeyId"].as_str().unwrap_or("").to_string();
            let secret = key["secretAccessKey"].as_str().unwrap_or("").to_string();
            tracing::info!(key = %key_name, "created garage API key");
            (key_id, secret)
        };

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

        // Delete key
        let keys = self.api_get("/v1/key?list").await?;
        if let Some(key) = keys
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .find(|k| k["name"].as_str() == Some(&key_name))
        {
            if let Some(key_id) = key["accessKeyId"].as_str() {
                self.api_delete(&format!("/v1/key?id={key_id}")).await?;
                tracing::info!(key = %key_name, "deleted garage API key");
            }
        }

        // Delete bucket (must be empty first)
        let buckets = self.api_get("/v1/bucket?list").await?;
        if let Some(bucket) = buckets.as_array().unwrap_or(&vec![]).iter().find(|b| {
            b["globalAliases"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .any(|a| a.as_str() == Some(&bucket_name))
        }) {
            if let Some(bucket_id) = bucket["id"].as_str() {
                self.api_delete(&format!("/v1/bucket?id={bucket_id}"))
                    .await?;
                tracing::info!(bucket = %bucket_name, "deleted garage bucket");
            }
        }

        Ok(())
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        let active_buckets: std::collections::HashSet<String> =
            active.iter().map(Self::bucket_name).collect();

        let buckets = self.api_get("/v1/bucket?list").await?;
        for bucket in buckets.as_array().unwrap_or(&vec![]) {
            let empty = vec![];
            let aliases = bucket["globalAliases"].as_array().unwrap_or(&empty);
            for alias in aliases {
                if let Some(name) = alias.as_str() {
                    if name.starts_with("kennel-") && !active_buckets.contains(name) {
                        tracing::info!(bucket = %name, "deleting orphaned garage bucket");
                        if let Some(id) = bucket["id"].as_str() {
                            let _ = self.api_delete(&format!("/v1/bucket?id={id}")).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
