use kennel_config::constants;
use kennel_store::Store;
use kennel_supervisor::Supervisor;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub fn create_builder_config(
    store: Arc<Store>,
    deploy_tx: mpsc::Sender<kennel_deployer::DeploymentRequest>,
) -> kennel_builder::BuilderConfig {
    kennel_builder::BuilderConfig {
        store,
        deploy_tx,
        max_concurrent_builds: std::env::var("MAX_CONCURRENT_BUILDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(constants::DEFAULT_MAX_CONCURRENT_BUILDS),
        work_dir: std::env::var("WORK_DIR").unwrap_or_else(|_| constants::DEFAULT_WORK_DIR.into()),
    }
}

pub fn create_deployer_config(
    store: Arc<Store>,
    supervisor: Arc<Mutex<Supervisor>>,
    dns_manager: Option<Arc<kennel_dns::DnsManager>>,
    resource_providers: Vec<Arc<dyn kennel_provision::ResourceProvider>>,
    base_domain: String,
) -> kennel_deployer::DeployerConfig {
    kennel_deployer::DeployerConfig {
        store,
        supervisor,
        dns_manager,
        resource_providers,
        vault_endpoint: std::env::var("VAULT_ENDPOINT").ok(),
        base_domain,
    }
}

pub fn create_router_config(store: Arc<Store>) -> kennel_router::RouterConfig {
    kennel_router::RouterConfig {
        store,
        bind_addr: std::env::var("ROUTER_ADDR")
            .unwrap_or_else(|_| constants::DEFAULT_ROUTER_ADDR.into()),
        tls_enabled: std::env::var("TLS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        acme_email: std::env::var("ACME_EMAIL").ok(),
        acme_production: std::env::var("ACME_PRODUCTION")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        acme_cache_dir: std::env::var("ACME_CACHE_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    }
}
