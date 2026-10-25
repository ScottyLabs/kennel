mod caddy;
mod cloudflare;
mod custom_domains;
mod deploy;
mod forgejo;
mod handlers;
mod health;
mod http;
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
use sea_orm::Database;
use sea_orm_migration::MigratorTrait as _;
use std::sync::Arc;
use store::Store;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vault::VaultClient;

pub struct AppState {
    pub store: Store,
    /// Wakes the reconciler when a deploy request may be actionable
    pub reconcile_signal: Notify,
    pub config: AppConfig,
    pub providers: Vec<kennel_provision::Provider>,
    pub forgejo: ForgejoClient,
    pub cloudflare: Option<CloudflareClient>,
    pub vault: Option<VaultClient>,
}

pub struct AppConfig {
    pub api_host: String,
    pub api_port: u16,
    pub ephemeral_domain: String,
    pub caddy_admin_url: String,
    pub webhook_secret: String,
    pub nixbot_token: String,
    pub custom_domains_file: Option<String>,
    pub grafana_url: Option<String>,
}

impl AppConfig {
    fn from_env() -> Result<Self> {
        Ok(Self {
            api_host: std::env::var("API_HOST")
                .unwrap_or_else(|_| kennel_config::constants::DEFAULT_API_HOST.into()),
            api_port: std::env::var("API_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(kennel_config::constants::DEFAULT_API_PORT),
            ephemeral_domain: std::env::var("EPHEMERAL_DOMAIN")
                .unwrap_or_else(|_| kennel_config::constants::DEFAULT_EPHEMERAL_DOMAIN.into()),
            caddy_admin_url: std::env::var("CADDY_ADMIN_URL")
                .unwrap_or_else(|_| "http://localhost:2019".into()),
            webhook_secret: read_secret_file("WEBHOOK_SECRET_FILE")?,
            nixbot_token: read_secret_file("NIXBOT_TOKEN_FILE")?,
            custom_domains_file: std::env::var("CUSTOM_DOMAINS_FILE").ok(),
            grafana_url: std::env::var("GRAFANA_URL").ok(),
        })
    }
}

fn read_secret_file(var: &str) -> Result<String> {
    let path = std::env::var(var).map_err(|_| anyhow::anyhow!("{var} must be set"))?;
    let value = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {var} from {path}: {e}"))?;
    Ok(value.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().json().init();

    let config = AppConfig::from_env()?;
    let db_path = std::env::var("DATABASE_PATH")
        .unwrap_or_else(|_| kennel_config::constants::DEFAULT_DB_PATH.into());
    let db_url = format!("sqlite://{}?mode=rwc", db_path);

    let db = Database::connect(&db_url).await?;
    migration::Migrator::up(&db, None).await?;

    let store = Store::new(db);
    let providers = build_providers();
    let forgejo = build_forgejo_client()?;
    let cloudflare = build_cloudflare_client()?;
    let vault = vault::build_from_env()?;

    let state = Arc::new(AppState {
        store,
        reconcile_signal: Notify::new(),
        config,
        providers,
        forgejo,
        cloudflare,
        vault,
    });

    let cancel = CancellationToken::new();

    reconcile::run_once(&state).await?;

    let reconcile_handle = tokio::spawn(reconcile::run_loop(state.clone(), cancel.clone()));

    let app = http::router(state.clone());
    let addr = format!("{}:{}", state.config.api_host, state.config.api_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(signal::shutdown_signal())
        .await?;

    cancel.cancel();
    let _ = reconcile_handle.await;

    tracing::info!("shutdown complete");
    Ok(())
}

fn build_cloudflare_client() -> Result<Option<CloudflareClient>> {
    let Ok(token) = std::env::var("CLOUDFLARE_API_TOKEN") else {
        return Ok(None);
    };
    let Ok(zones_json) = std::env::var("CLOUDFLARE_ZONES_JSON") else {
        return Ok(None);
    };
    let Ok(tunnel_id) = std::env::var("KENNEL_TUNNEL_ID") else {
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

    tracing::info!(zones = zones.len(), tunnel = %tunnel_id, "cloudflare DNS automation enabled");
    Ok(Some(CloudflareClient::new(token, zones, tunnel_id)))
}

fn build_forgejo_client() -> Result<ForgejoClient> {
    let token_file = std::env::var("FORGEJO_API_TOKEN_FILE")
        .map_err(|_| anyhow::anyhow!("FORGEJO_API_TOKEN_FILE must be set"))?;
    let token = std::fs::read_to_string(&token_file)
        .map_err(|e| anyhow::anyhow!("failed to read forgejo token from {token_file}: {e}"))?
        .trim()
        .to_string();
    let api_base =
        std::env::var("FORGEJO_API_URL").unwrap_or_else(|_| "https://git.cmu.dev/api/v1".into());
    Ok(ForgejoClient::new(api_base, token))
}

fn build_providers() -> Vec<kennel_provision::Provider> {
    let mut providers = Vec::new();

    if let Ok(socket_dir) = std::env::var("POSTGRES_SOCKET_DIR") {
        providers.push(kennel_provision::Provider::Postgres(
            kennel_provision::postgres::PostgresProvider::new(socket_dir),
        ));
    }

    if let Ok(socket_path) = std::env::var("VALKEY_SOCKET_PATH") {
        providers.push(kennel_provision::Provider::Valkey(
            kennel_provision::valkey::ValkeyProvider::new(socket_path),
        ));
    }

    if let Ok(admin_endpoint) = std::env::var("GARAGE_ADMIN_ENDPOINT") {
        let s3_endpoint =
            std::env::var("GARAGE_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:3900".into());
        let admin_token = std::env::var("GARAGE_ADMIN_TOKEN")
            .expect("GARAGE_ADMIN_TOKEN required when GARAGE_ADMIN_ENDPOINT is set");
        providers.push(kennel_provision::Provider::Garage(
            kennel_provision::garage::GarageProvider::new(admin_endpoint, s3_endpoint, admin_token),
        ));
    }

    providers.push(kennel_provision::Provider::Sqlite(
        kennel_provision::sqlite::SqliteProvider,
    ));

    providers
}
