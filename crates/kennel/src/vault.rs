use anyhow::{Result, anyhow, ensure};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Minimal Vault/OpenBao client for writes that secretspec's resolver
/// cannot perform. Authenticates via AppRole (the same one the kennel
/// systemd unit uses for everything else) and caches the resulting token.
pub struct VaultClient {
    client: Client,
    /// Vault/OpenBao server URL.
    url: String,
    /// KV v2 mount path, e.g. "secret".
    mount: String,
    role_id: String,
    secret_id: String,
    token: Mutex<Option<(String, Instant)>>,
}

#[derive(Deserialize)]
struct LoginResponse {
    auth: LoginAuth,
}

#[derive(Deserialize)]
struct LoginAuth {
    client_token: String,
    lease_duration: u64,
}

#[derive(Serialize)]
struct WriteBody<'a> {
    data: &'a serde_json::Value,
}

impl VaultClient {
    pub fn new(url: String, mount: String, role_id: String, secret_id: String) -> Self {
        Self {
            client: Client::new(),
            url: url.trim_end_matches('/').to_string(),
            mount,
            role_id,
            secret_id,
            token: Mutex::new(None),
        }
    }

    async fn token(&self) -> Result<String> {
        {
            let cached = self.token.lock().await;
            if let Some((tok, expires_at)) = cached.as_ref()
                && Instant::now() + Duration::from_secs(30) < *expires_at
            {
                return Ok(tok.clone());
            }
        }

        let url = format!("{}/v1/auth/approle/login", self.url);
        let body = serde_json::json!({
            "role_id": self.role_id,
            "secret_id": self.secret_id,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        ensure!(
            resp.status().is_success(),
            "vault approle login returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        let body: LoginResponse = resp.json().await?;
        let expires_at = Instant::now() + Duration::from_secs(body.auth.lease_duration);
        let mut cached = self.token.lock().await;
        *cached = Some((body.auth.client_token.clone(), expires_at));
        Ok(body.auth.client_token)
    }

    /// Write a single KV v2 entry at `{mount}/data/{path}` with body `{ data: value }`.
    pub async fn write(&self, path: &str, value: &serde_json::Value) -> Result<()> {
        let token = self.token().await?;
        let url = format!("{}/v1/{}/data/{}", self.url, self.mount, path);
        let resp = self
            .client
            .post(&url)
            .header("X-Vault-Token", &token)
            .json(&WriteBody { data: value })
            .send()
            .await?;
        ensure!(
            resp.status().is_success(),
            "vault write {path} returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        Ok(())
    }
}

pub fn build_from_env() -> Result<Option<VaultClient>> {
    let Ok(endpoint) = std::env::var("VAULT_ENDPOINT") else {
        return Ok(None);
    };
    // VAULT_ENDPOINT is the full secretspec provider URI of the form
    // vault://<host>/<mount>?<query>, e.g. ?auth=approle.
    let stripped = endpoint
        .strip_prefix("vault://")
        .or_else(|| endpoint.strip_prefix("openbao://"))
        .ok_or_else(|| anyhow!("VAULT_ENDPOINT must start with vault:// or openbao://"))?;
    let without_query = stripped.split('?').next().unwrap_or(stripped);
    let (host, mount) = without_query
        .split_once('/')
        .ok_or_else(|| anyhow!("VAULT_ENDPOINT must include a mount path"))?;
    ensure!(!host.is_empty(), "VAULT_ENDPOINT host is empty");
    ensure!(!mount.is_empty(), "VAULT_ENDPOINT mount is empty");
    let base = format!("https://{host}");

    let role_id = std::env::var("VAULT_ROLE_ID")
        .map_err(|_| anyhow!("VAULT_ROLE_ID required when VAULT_ENDPOINT is set"))?;
    let secret_id = std::env::var("VAULT_SECRET_ID")
        .map_err(|_| anyhow!("VAULT_SECRET_ID required when VAULT_ENDPOINT is set"))?;

    tracing::info!(url = %base, mount = %mount, "vault client enabled");
    Ok(Some(VaultClient::new(
        base,
        mount.to_string(),
        role_id,
        secret_id,
    )))
}
