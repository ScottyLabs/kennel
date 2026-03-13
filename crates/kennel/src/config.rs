use kennel_config::constants;
use kennel_store::Store;
use std::sync::Arc;

pub fn create_router_config(store: Arc<Store>) -> kennel_router::RouterConfig {
    kennel_router::RouterConfig {
        store,
        bind_addr: std::env::var("ROUTER_ADDR")
            .unwrap_or_else(|_| constants::DEFAULT_ROUTER_ADDR.into()),
        tls_enabled: std::env::var("TLS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        acme_email: std::env::var("ACME_EMAIL").ok(),
        acme_production: std::env::var("ACME_STAGING")
            .map(|v| v != "true" && v != "1")
            .unwrap_or(true),
        acme_cache_dir: std::env::var("ACME_CACHE_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    }
}
