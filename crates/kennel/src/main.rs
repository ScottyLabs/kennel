mod build;
mod caddy;
mod cloudflare;
mod deploy;
mod forgejo;
mod handlers;
mod http;
mod keycloak;
mod reconcile;
mod secrets;
mod signal;
pub mod store;
mod systemd;
mod teardown;
mod vault;

use anyhow::Result;
use cloudflare::CloudflareClient;
use forgejo::ForgejoClient;
use keycloak::KeycloakClient;
use sea_orm::Database;
use sea_orm_migration::MigratorTrait as _;
use std::sync::Arc;
use store::Store;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vault::VaultClient;

pub struct AppState {
    pub store: Store,
    pub signal: Arc<Notify>,
    pub config: AppConfig,
    pub providers: Vec<kennel_provision::Provider>,
    pub forgejo: ForgejoClient,
    pub cloudflare: Option<CloudflareClient>,
    pub keycloak: Option<KeycloakClient>,
    pub vault: Option<VaultClient>,
}

pub struct AppConfig {
    pub api_host: String,
    pub api_port: u16,
    pub ephemeral_domain: String,
    pub work_dir: String,
    pub caddy_admin_url: String,
    pub max_concurrent_builds: usize,
    pub webhook_secret: String,
}

impl AppConfig {
    fn from_env() -> Result<Self> {
        let webhook_secret_file =
            dotenvy::var("WEBHOOK_SECRET_FILE").expect("WEBHOOK_SECRET_FILE must be set");
        let webhook_secret = std::fs::read_to_string(&webhook_secret_file)
            .map_err(|e| {
                anyhow::anyhow!("failed to read webhook secret from {webhook_secret_file}: {e}")
            })?
            .trim()
            .to_string();

        Ok(Self {
            api_host: dotenvy::var("API_HOST")
                .unwrap_or_else(|_| kennel_config::constants::DEFAULT_API_HOST.into()),
            api_port: dotenvy::var("API_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(kennel_config::constants::DEFAULT_API_PORT),
            ephemeral_domain: dotenvy::var("EPHEMERAL_DOMAIN")
                .unwrap_or_else(|_| kennel_config::constants::DEFAULT_EPHEMERAL_DOMAIN.into()),
            work_dir: dotenvy::var("WORK_DIR")
                .unwrap_or_else(|_| kennel_config::constants::DEFAULT_WORK_DIR.into()),
            caddy_admin_url: dotenvy::var("CADDY_ADMIN_URL")
                .unwrap_or_else(|_| "http://localhost:2019".into()),
            max_concurrent_builds: dotenvy::var("MAX_CONCURRENT_BUILDS")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(kennel_config::constants::DEFAULT_MAX_CONCURRENT_BUILDS),
            webhook_secret,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt().json().init();

    let config = AppConfig::from_env()?;
    let db_path = dotenvy::var("DATABASE_PATH")
        .unwrap_or_else(|_| kennel_config::constants::DEFAULT_DB_PATH.into());
    let db_url = format!("sqlite://{}?mode=rwc", db_path);

    let db = Database::connect(&db_url).await?;
    migration::Migrator::up(&db, None).await?;

    let store = Store::new(db);
    let providers = build_providers();
    let forgejo = build_forgejo_client()?;
    let cloudflare = build_cloudflare_client()?;
    let keycloak = build_keycloak_client()?;
    let vault = vault::build_from_env()?;
    let signal = Arc::new(Notify::new());

    let state = Arc::new(AppState {
        store,
        signal: signal.clone(),
        config,
        providers,
        forgejo,
        cloudflare,
        keycloak,
        vault,
    });

    let cancel = CancellationToken::new();

    reconcile::run_once(&state).await?;

    let build_handle = tokio::spawn(build::run_worker(state.clone(), cancel.clone()));
    let reconcile_handle = tokio::spawn(reconcile::run_loop(state.clone(), cancel.clone()));

    signal.notify_one();

    let app = http::router(state.clone());
    let addr = format!("{}:{}", state.config.api_host, state.config.api_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(signal::shutdown_signal())
        .await?;

    cancel.cancel();
    let _ = tokio::join!(build_handle, reconcile_handle);

    tracing::info!("shutdown complete");
    Ok(())
}

fn build_cloudflare_client() -> Result<Option<CloudflareClient>> {
    let Ok(token) = dotenvy::var("CLOUDFLARE_API_TOKEN") else {
        return Ok(None);
    };
    let Ok(zones_json) = dotenvy::var("CLOUDFLARE_ZONES_JSON") else {
        return Ok(None);
    };
    let Ok(public_ip) = dotenvy::var("KENNEL_PUBLIC_IP") else {
        return Ok(None);
    };

    let token = token.trim().to_string();
    if token.is_empty() {
        return Ok(None);
    }

    let zones: std::collections::HashMap<String, String> = serde_json::from_str(&zones_json)
        .map_err(|e| anyhow::anyhow!("CLOUDFLARE_ZONES_JSON is not a valid JSON object: {e}"))?;
    if zones.is_empty() {
        return Ok(None);
    }

    tracing::info!(zones = zones.len(), public_ip = %public_ip, "cloudflare DNS automation enabled");
    Ok(Some(CloudflareClient::new(token, zones, public_ip)))
}

fn build_keycloak_client() -> Result<Option<KeycloakClient>> {
    let Ok(url) = dotenvy::var("KEYCLOAK_URL") else {
        return Ok(None);
    };
    let realm = dotenvy::var("KEYCLOAK_REALM").unwrap_or_else(|_| "scottylabs".into());
    let admin_client_id = dotenvy::var("KEYCLOAK_ADMIN_CLIENT_ID").map_err(|_| {
        anyhow::anyhow!("KEYCLOAK_ADMIN_CLIENT_ID required when KEYCLOAK_URL is set")
    })?;
    let secret_file = dotenvy::var("KEYCLOAK_ADMIN_CLIENT_SECRET_FILE").map_err(|_| {
        anyhow::anyhow!("KEYCLOAK_ADMIN_CLIENT_SECRET_FILE required when KEYCLOAK_URL is set")
    })?;
    let admin_client_secret = std::fs::read_to_string(&secret_file)
        .map_err(|e| {
            anyhow::anyhow!("failed to read keycloak admin secret from {secret_file}: {e}")
        })?
        .trim()
        .to_string();

    tracing::info!(url = %url, realm = %realm, "keycloak admin client enabled");
    Ok(Some(KeycloakClient::new(
        url,
        realm,
        admin_client_id,
        admin_client_secret,
    )))
}

fn build_forgejo_client() -> Result<ForgejoClient> {
    let token_file = dotenvy::var("FORGEJO_API_TOKEN_FILE")
        .map_err(|_| anyhow::anyhow!("FORGEJO_API_TOKEN_FILE must be set"))?;
    let token = std::fs::read_to_string(&token_file)
        .map_err(|e| anyhow::anyhow!("failed to read forgejo token from {token_file}: {e}"))?
        .trim()
        .to_string();
    let api_base =
        dotenvy::var("FORGEJO_API_URL").unwrap_or_else(|_| "https://codeberg.org/api/v1".into());
    Ok(ForgejoClient::new(api_base, token))
}

fn build_providers() -> Vec<kennel_provision::Provider> {
    let mut providers = Vec::new();

    if let Ok(socket_dir) = dotenvy::var("POSTGRES_SOCKET_DIR") {
        providers.push(kennel_provision::Provider::Postgres(
            kennel_provision::postgres::PostgresProvider::new(socket_dir),
        ));
    }

    if let Ok(socket_path) = dotenvy::var("VALKEY_SOCKET_PATH") {
        providers.push(kennel_provision::Provider::Valkey(
            kennel_provision::valkey::ValkeyProvider::new(socket_path),
        ));
    }

    if let Ok(admin_endpoint) = dotenvy::var("GARAGE_ADMIN_ENDPOINT") {
        let s3_endpoint =
            dotenvy::var("GARAGE_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:3900".into());
        let admin_token = dotenvy::var("GARAGE_ADMIN_TOKEN")
            .expect("GARAGE_ADMIN_TOKEN required when GARAGE_ADMIN_ENDPOINT is set");
        providers.push(kennel_provision::Provider::Garage(
            kennel_provision::garage::GarageProvider::new(admin_endpoint, s3_endpoint, admin_token),
        ));
    }

    providers
}
