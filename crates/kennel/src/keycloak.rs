use crate::AppState;
use anyhow::{Result, anyhow, ensure};
use kennel_config::ServiceConfig;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Keycloak admin REST client. Uses client-credentials grant against the
/// realm's openid-connect token endpoint to obtain an admin token, which is
/// cached until 30 seconds before its expiry.
pub struct KeycloakClient {
    client: Client,
    /// Server URL, e.g. "https://idp.scottylabs.org".
    url: String,
    /// Realm to administer.
    realm: String,
    /// Service-account client_id used to authenticate against Keycloak.
    admin_client_id: String,
    admin_client_secret: String,
    token: Mutex<Option<(String, Instant)>>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// Subset of Keycloak's ClientRepresentation that we read and write.
/// Unknown fields are preserved on read via `extra` so PUTs don't drop them.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClientRepresentation {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "clientId")]
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    enabled: bool,
    #[serde(rename = "publicClient")]
    public_client: bool,
    #[serde(rename = "standardFlowEnabled")]
    standard_flow_enabled: bool,
    #[serde(rename = "directAccessGrantsEnabled")]
    direct_access_grants_enabled: bool,
    #[serde(rename = "redirectUris")]
    valid_redirect_uris: Vec<String>,
    #[serde(rename = "webOrigins")]
    web_origins: Vec<String>,
    #[serde(rename = "protocolMappers", skip_serializing_if = "Option::is_none")]
    protocol_mappers: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

fn groups_mapper() -> serde_json::Value {
    serde_json::json!({
        "name": "groups",
        "protocol": "openid-connect",
        "protocolMapper": "oidc-group-membership-mapper",
        "config": {
            "claim.name": "groups",
            "full.path": "true",
            "id.token.claim": "true",
            "access.token.claim": "true",
            "userinfo.token.claim": "true",
        },
    })
}

impl KeycloakClient {
    pub fn new(
        url: String,
        realm: String,
        admin_client_id: String,
        admin_client_secret: String,
    ) -> Self {
        Self {
            client: Client::new(),
            url: url.trim_end_matches('/').to_string(),
            realm,
            admin_client_id,
            admin_client_secret,
            token: Mutex::new(None),
        }
    }

    async fn admin_token(&self) -> Result<String> {
        {
            let cached = self.token.lock().await;
            if let Some((tok, expires_at)) = cached.as_ref() {
                if Instant::now() + Duration::from_secs(30) < *expires_at {
                    return Ok(tok.clone());
                }
            }
        }

        let url = format!(
            "{}/realms/{}/protocol/openid-connect/token",
            self.url, self.realm
        );
        let body = format!(
            "grant_type=client_credentials&client_id={}&client_secret={}",
            urlencoding::encode(&self.admin_client_id),
            urlencoding::encode(&self.admin_client_secret),
        );
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        ensure!(
            resp.status().is_success(),
            "keycloak token endpoint returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        let body: TokenResponse = resp.json().await?;
        let expires_at = Instant::now() + Duration::from_secs(body.expires_in);
        let mut cached = self.token.lock().await;
        *cached = Some((body.access_token.clone(), expires_at));
        Ok(body.access_token)
    }

    fn admin_base(&self) -> String {
        format!("{}/admin/realms/{}", self.url, self.realm)
    }

    async fn find_client(&self, client_id: &str) -> Result<Option<ClientRepresentation>> {
        let token = self.admin_token().await?;
        let url = format!(
            "{}/clients?clientId={}",
            self.admin_base(),
            urlencoding::encode(client_id)
        );
        let resp = self.client.get(&url).bearer_auth(&token).send().await?;
        ensure!(
            resp.status().is_success(),
            "keycloak find_client failed: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        let mut clients: Vec<ClientRepresentation> = resp.json().await?;
        Ok(clients.pop())
    }

    async fn put_client(&self, client: &ClientRepresentation) -> Result<()> {
        let token = self.admin_token().await?;
        let id = client
            .id
            .as_ref()
            .ok_or_else(|| anyhow!("cannot PUT client without an id"))?;
        let url = format!("{}/clients/{}", self.admin_base(), id);
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&token)
            .json(client)
            .send()
            .await?;
        ensure!(
            resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT,
            "keycloak update_client {} failed: {} {}",
            client.client_id,
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        Ok(())
    }

    /// Idempotently ensure a confidential client exists with exactly the given
    /// `valid_redirect_uris`. Creates with the standard groups protocol mapper
    /// if absent; updates redirect URIs if the existing set differs.
    pub async fn ensure_client(&self, client_id: &str, redirect_uris: &[String]) -> Result<()> {
        if let Some(mut existing) = self.find_client(client_id).await? {
            let existing_set: HashSet<&String> = existing.valid_redirect_uris.iter().collect();
            let desired_set: HashSet<&String> = redirect_uris.iter().collect();
            if existing_set == desired_set {
                return Ok(());
            }
            existing.valid_redirect_uris = redirect_uris.to_vec();
            existing.protocol_mappers = None;
            self.put_client(&existing).await?;
            tracing::info!(client_id, "updated keycloak client redirect URIs");
            return Ok(());
        }

        let new = ClientRepresentation {
            id: None,
            client_id: client_id.to_string(),
            name: Some(client_id.to_string()),
            enabled: true,
            public_client: false,
            standard_flow_enabled: true,
            direct_access_grants_enabled: false,
            valid_redirect_uris: redirect_uris.to_vec(),
            web_origins: vec!["+".to_string()],
            protocol_mappers: Some(vec![groups_mapper()]),
            extra: serde_json::Map::new(),
        };
        let token = self.admin_token().await?;
        let url = format!("{}/clients", self.admin_base());
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&new)
            .send()
            .await?;
        ensure!(
            resp.status() == StatusCode::CREATED,
            "keycloak create_client {} failed: {} {}",
            client_id,
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        tracing::info!(client_id, "created keycloak client");
        Ok(())
    }

    /// Fetch the current client_secret for an existing confidential client.
    pub async fn client_secret(&self, client_id: &str) -> Result<String> {
        let existing = self
            .find_client(client_id)
            .await?
            .ok_or_else(|| anyhow!("keycloak client {client_id} not found"))?;
        let id = existing
            .id
            .as_ref()
            .ok_or_else(|| anyhow!("keycloak client {client_id} has no id"))?;
        let token = self.admin_token().await?;
        let url = format!("{}/clients/{}/client-secret", self.admin_base(), id);
        let resp = self.client.get(&url).bearer_auth(&token).send().await?;
        ensure!(
            resp.status().is_success(),
            "keycloak client_secret {} failed: {} {}",
            client_id,
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        let body: serde_json::Value = resp.json().await?;
        body.get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("keycloak client_secret response missing 'value'"))
    }

    pub async fn add_redirect_uri(&self, client_id: &str, uri: &str) -> Result<()> {
        let mut existing = self
            .find_client(client_id)
            .await?
            .ok_or_else(|| anyhow!("keycloak client {client_id} not found"))?;
        if existing.valid_redirect_uris.iter().any(|u| u == uri) {
            return Ok(());
        }
        existing.valid_redirect_uris.push(uri.to_string());
        existing.protocol_mappers = None;
        self.put_client(&existing).await?;
        tracing::info!(client_id, uri, "added keycloak redirect URI");
        Ok(())
    }

    pub async fn remove_redirect_uri(&self, client_id: &str, uri: &str) -> Result<()> {
        let Some(mut existing) = self.find_client(client_id).await? else {
            return Ok(());
        };
        let before = existing.valid_redirect_uris.len();
        existing.valid_redirect_uris.retain(|u| u != uri);
        if existing.valid_redirect_uris.len() == before {
            return Ok(());
        }
        existing.protocol_mappers = None;
        self.put_client(&existing).await?;
        tracing::info!(client_id, uri, "removed keycloak redirect URI");
        Ok(())
    }
}

/// Per-service reconcile invoked on every deploy. Idempotently ensures the
/// project's prod and staging Keycloak clients exist with the correct base
/// `valid_redirect_uris`, then for PR branches additively registers the
/// preview deployment URL on the staging client.
pub async fn reconcile(
    state: &AppState,
    project_slug: &str,
    service_name: &str,
    branch: &str,
    svc_config: &ServiceConfig,
) -> Result<()> {
    let Some(kc) = state.keycloak.as_ref() else {
        return Ok(());
    };
    let Some(oidc) = svc_config.oidc.as_ref() else {
        return Ok(());
    };

    let prod_host = format!("{project_slug}-{service_name}-main.scottylabs.net");
    let staging_host = format!("{project_slug}-{service_name}-staging.scottylabs.net");

    let mut prod_uris: Vec<String> = oidc
        .redirect_paths
        .iter()
        .map(|p| format!("https://{prod_host}{p}"))
        .collect();
    if let Some(custom) = svc_config.custom_domain.as_ref() {
        for path in &oidc.redirect_paths {
            prod_uris.push(format!("https://{custom}{path}"));
        }
    }
    let staging_uris: Vec<String> = oidc
        .redirect_paths
        .iter()
        .map(|p| format!("https://{staging_host}{p}"))
        .collect();

    kc.ensure_client(project_slug, &prod_uris).await?;
    let staging_client_id = format!("{project_slug}-staging");
    kc.ensure_client(&staging_client_id, &staging_uris).await?;

    if let Some(pr) = crate::forgejo::pr_number_from_branch(branch) {
        let preview_host = format!("{project_slug}-{service_name}-pr-{pr}.scottylabs.net");
        for path in &oidc.redirect_paths {
            let uri = format!("https://{preview_host}{path}");
            kc.add_redirect_uri(&staging_client_id, &uri).await?;
        }
    }

    if let Some(vault) = state.vault.as_ref() {
        let prod_secret = kc.client_secret(project_slug).await?;
        write_oidc_secrets(vault, project_slug, "prod", project_slug, &prod_secret).await?;

        let staging_secret = kc.client_secret(&staging_client_id).await?;
        for profile in ["staging", "preview"] {
            write_oidc_secrets(
                vault,
                project_slug,
                profile,
                &staging_client_id,
                &staging_secret,
            )
            .await?;
        }
    }

    Ok(())
}

async fn write_oidc_secrets(
    vault: &crate::vault::VaultClient,
    project_slug: &str,
    profile: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<()> {
    vault
        .write(
            &format!("secretspec/{project_slug}/{profile}/OIDC_CLIENT_ID"),
            &serde_json::json!({ "value": client_id }),
        )
        .await?;
    vault
        .write(
            &format!("secretspec/{project_slug}/{profile}/OIDC_CLIENT_SECRET"),
            &serde_json::json!({ "value": client_secret }),
        )
        .await?;
    Ok(())
}

/// Per-service teardown for PR branches. Removes the preview deployment URL
/// from the staging client. No-op on non-PR branches.
pub async fn teardown_preview(
    state: &AppState,
    project_slug: &str,
    service_name: &str,
    branch: &str,
    svc_config: &ServiceConfig,
) -> Result<()> {
    let Some(kc) = state.keycloak.as_ref() else {
        return Ok(());
    };
    let Some(oidc) = svc_config.oidc.as_ref() else {
        return Ok(());
    };
    let Some(pr) = crate::forgejo::pr_number_from_branch(branch) else {
        return Ok(());
    };

    let staging_client_id = format!("{project_slug}-staging");
    let preview_host = format!("{project_slug}-{service_name}-pr-{pr}.scottylabs.net");
    for path in &oidc.redirect_paths {
        let uri = format!("https://{preview_host}{path}");
        kc.remove_redirect_uri(&staging_client_id, &uri).await?;
    }
    Ok(())
}
