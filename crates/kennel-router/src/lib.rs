mod acme;
mod error;
mod handler;
mod health;
mod proxy;
mod static_serve;
mod table;
mod tls;

pub use acme::{create_acme_state, run_acme_event_loop};
pub use error::{Result, RouterError};
pub use health::run_health_monitor;
pub use table::{Route, RouteTarget, RoutingTable};
pub use tls::serve_with_tls;

#[derive(Debug, Clone)]
pub enum RouterUpdate {
    DeploymentActive {
        deployment_id: i32,
        domain: String,
        port: Option<u16>,
        store_path: Option<String>,
        spa: bool,
    },
    DeploymentRemoved {
        domain: String,
    },
    FullReload,
}

use axum::Router;
use kennel_store::Store;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

pub struct RouterConfig {
    pub store: Arc<Store>,
    pub bind_addr: String,
    pub tls_enabled: bool,
    pub acme_email: Option<String>,
    pub acme_production: bool,
    pub acme_cache_dir: Option<std::path::PathBuf>,
}

pub async fn run_router(
    config: RouterConfig,
    update_rx: tokio::sync::broadcast::Receiver<RouterUpdate>,
) -> Result<()> {
    info!("Starting router on {}", config.bind_addr);

    let routing_table = Arc::new(RoutingTable::new());

    let active_deployments = config
        .store
        .deployments()
        .list_active_with_services()
        .await
        .map_err(|e| RouterError::Other(anyhow::anyhow!(e)))?;

    routing_table
        .load_from_deployments_with_services(active_deployments)
        .await?;

    info!("Loaded {} routes", routing_table.len().await);

    let table_clone = routing_table.clone();
    let store_clone = config.store.clone();
    tokio::spawn(async move {
        run_update_handler(table_clone, store_clone, update_rx).await;
    });

    let app = Router::new()
        .fallback(handler::route_request)
        .with_state(routing_table);

    if config.tls_enabled {
        let email = config.acme_email.ok_or_else(|| {
            RouterError::Other(anyhow::anyhow!("ACME email required when TLS is enabled"))
        })?;

        let cache_dir = config
            .acme_cache_dir
            .unwrap_or_else(|| std::path::PathBuf::from(kennel_config::constants::ACME_CACHE_DIR));

        let domains = get_all_domains(&config.store).await?;

        let acme_state = create_acme_state(domains, email, cache_dir, config.acme_production);

        let addr: std::net::SocketAddr = config
            .bind_addr
            .parse()
            .map_err(|e| RouterError::Other(anyhow::anyhow!("Invalid bind address: {}", e)))?;

        info!("Router starting with TLS on {}", addr);

        serve_with_tls(app, addr, acme_state)
            .await
            .map_err(|e| RouterError::Other(anyhow::anyhow!("TLS server error: {}", e)))?;
    } else {
        let listener = TcpListener::bind(&config.bind_addr).await?;
        info!("Router listening on {} (HTTP only)", config.bind_addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await?;
    }

    Ok(())
}

async fn get_all_domains(store: &Store) -> Result<Vec<String>> {
    let deployments = store
        .deployments()
        .list_active_with_services()
        .await
        .map_err(|e| RouterError::Other(anyhow::anyhow!(e)))?;

    let mut domains = Vec::new();
    for (deployment, service_opt) in deployments {
        if let Some(service) = service_opt
            && let Some(custom_domain) = service.custom_domain
        {
            domains.push(custom_domain);
        }
        domains.push(deployment.domain);
    }

    domains.sort();
    domains.dedup();

    Ok(domains)
}

async fn run_update_handler(
    table: Arc<RoutingTable>,
    store: Arc<Store>,
    mut update_rx: tokio::sync::broadcast::Receiver<RouterUpdate>,
) {
    use std::path::PathBuf;
    use tokio::time::interval;

    info!("Starting routing table update handler");

    let mut reload_interval = interval(kennel_config::constants::ROUTER_RELOAD_INTERVAL);

    loop {
        tokio::select! {
            Ok(update) = update_rx.recv() => {
                match update {
                    RouterUpdate::DeploymentActive { deployment_id, domain, port, store_path, spa } => {
                        info!("Updating route for domain: {} (deployment {})", domain, deployment_id);

                        let target = if let Some(port) = port {
                            RouteTarget::Service { port }
                        } else if let Some(path_str) = store_path {
                            RouteTarget::StaticSite {
                                path: PathBuf::from(path_str),
                                spa,
                            }
                        } else {
                            error!("Invalid deployment update: no port or store_path");
                            continue;
                        };

                        table.insert(domain, Route {
                            target,
                            deployment_id,
                        }).await;
                    }
                    RouterUpdate::DeploymentRemoved { domain } => {
                        info!("Removing route for domain: {}", domain);
                        table.remove(&domain).await;
                    }
                    RouterUpdate::FullReload => {
                        info!("Full routing table reload requested");
                        if let Err(e) = reload_routing_table(&table, &store).await {
                            error!("Failed to reload routing table: {}", e);
                        }
                    }
                }
            }
            _ = reload_interval.tick() => {
                info!("Periodic routing table reload");
                if let Err(e) = reload_routing_table(&table, &store).await {
                    error!("Failed to reload routing table: {}", e);
                }
            }
        }
    }
}

async fn reload_routing_table(table: &RoutingTable, store: &Store) -> Result<()> {
    let active_deployments = store
        .deployments()
        .list_active_with_services()
        .await
        .map_err(|e| RouterError::Other(anyhow::anyhow!(e)))?;

    table
        .load_from_deployments_with_services(active_deployments)
        .await?;

    info!("Reloaded routing table with {} routes", table.len().await);

    Ok(())
}
