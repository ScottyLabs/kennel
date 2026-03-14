mod acme;
mod error;
mod handler;
mod proxy;
mod static_serve;
mod table;
mod tls;

pub use acme::{create_acme_state, run_acme_event_loop};
pub use error::{Result, RouterError};
pub use handler::RouterState;
pub use table::{Route, RouteTarget, RoutingTable};
pub use tls::serve_with_tls;

use axum::Router;
use kennel_store::Store;
use kennel_supervisor::SupervisorEvent;
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
    event_rx: tokio::sync::broadcast::Receiver<SupervisorEvent>,
) -> Result<()> {
    info!("Starting router on {}", config.bind_addr);

    let routing_table = Arc::new(RoutingTable::new());

    // Load static site routes from the database on startup.
    // Service routes are populated via supervisor events.
    let deployed = config
        .store
        .deployments()
        .list_deployed_with_services()
        .await
        .map_err(|e| RouterError::Other(anyhow::anyhow!(e)))?;

    routing_table
        .load_static_sites_from_deployments(deployed)
        .await?;

    info!("Loaded {} initial routes", routing_table.len().await);

    let table_for_handler = routing_table.clone();
    let store_clone = config.store.clone();
    tokio::spawn(async move {
        run_update_handler(table_for_handler, store_clone, event_rx).await;
    });

    let state = RouterState {
        table: routing_table,
        http_client: reqwest::Client::new(),
    };

    let app = Router::new()
        .fallback(handler::route_request)
        .with_state(state);

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
            .map_err(|e| RouterError::Other(anyhow::anyhow!("Invalid bind address: {e}")))?;

        info!("Router starting with TLS on {}", addr);

        serve_with_tls(app, addr, acme_state)
            .await
            .map_err(|e| RouterError::Other(anyhow::anyhow!("TLS server error: {e}")))?;
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
        .list_deployed_with_services()
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
    mut event_rx: tokio::sync::broadcast::Receiver<SupervisorEvent>,
) {
    use std::path::PathBuf;
    use tokio::time::interval;

    info!("Starting routing table update handler");

    let mut reload_interval = interval(kennel_config::constants::ROUTER_RELOAD_INTERVAL);

    loop {
        tokio::select! {
            result = event_rx.recv() => { match result {
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event receiver lagged, reloading routes");
                    if let Err(e) = reload_static_routes(&table, &store).await {
                        error!("Failed to reload routes after lag: {e}");
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Ok(event) => {
                match event {
                    SupervisorEvent::ProcessReady { name, port, store_path } => {
                        info!("Process ready: {name}");

                        // Look up the deployment by process name to get
                        // the domain and deployment ID.
                        if let Some(port) = port {
                            // Service deployment -- use the port the supervisor
                            // reported.
                            if let Ok(Some((deployment, _service))) = find_deployment_by_process_name(&store, &name).await {
                                table.insert(
                                    deployment.domain.clone(),
                                    Route {
                                        target: RouteTarget::Service { port },
                                        deployment_id: deployment.id,
                                    },
                                ).await;
                            }
                        } else if let Some(path_str) = store_path {
                            // Static site -- use the store path.
                            if let Ok(Some((deployment, service))) = find_deployment_by_process_name(&store, &name).await {
                                let spa = service.map(|s| s.spa).unwrap_or(false);
                                table.insert(
                                    deployment.domain.clone(),
                                    Route {
                                        target: RouteTarget::StaticSite {
                                            path: PathBuf::from(path_str),
                                            spa,
                                        },
                                        deployment_id: deployment.id,
                                    },
                                ).await;
                            }
                        }
                    }
                    SupervisorEvent::ProcessUnhealthy { name }
                    | SupervisorEvent::ProcessStopped { name } => {
                        if let Ok(Some((deployment, _service))) = find_deployment_by_process_name(&store, &name).await {
                            table.remove(&deployment.domain).await;
                        }
                    }
                    SupervisorEvent::ProcessHealthy { name, port } => {
                        if let Some(port) = port
                            && let Ok(Some((deployment, _service))) = find_deployment_by_process_name(&store, &name).await {
                                table.insert(
                                    deployment.domain.clone(),
                                    Route {
                                        target: RouteTarget::Service { port },
                                        deployment_id: deployment.id,
                                    },
                                ).await;
                            }
                    }
                    _ => {}
                }
            } } }
            _ = reload_interval.tick() => {
                if let Err(e) = reload_static_routes(&table, &store).await {
                    error!("Failed to reload routing table: {e}");
                }
            }
        }
    }
}

async fn find_deployment_by_process_name(
    store: &Store,
    process_name: &str,
) -> std::result::Result<
    Option<(entity::deployments::Model, Option<entity::services::Model>)>,
    anyhow::Error,
> {
    Ok(store
        .deployments()
        .find_deployed_by_process_name(process_name)
        .await?)
}

async fn reload_static_routes(table: &RoutingTable, store: &Store) -> Result<()> {
    let deployed = store
        .deployments()
        .list_deployed_with_services()
        .await
        .map_err(|e| RouterError::Other(anyhow::anyhow!(e)))?;

    table.load_static_sites_from_deployments(deployed).await?;

    Ok(())
}
