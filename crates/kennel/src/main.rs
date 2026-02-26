use kennel_config::constants;
use kennel_deployer::PortAllocator;
use kennel_store::Store;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let db = Database::connect(&database_url).await?;

    // Run migrations
    migration::Migrator::up(&db, None).await?;

    let store = Arc::new(Store::new(db));

    let (build_tx, build_rx) = mpsc::channel(1000);
    let (deploy_tx, deploy_rx) = mpsc::channel(100);
    let (teardown_tx, teardown_rx) = mpsc::channel(100);
    let (router_update_tx, router_update_rx) = tokio::sync::broadcast::channel(100);

    let webhook_config = kennel_webhook::WebhookConfig {
        store: store.clone(),
        build_tx,
    };

    let builder_config = kennel_builder::BuilderConfig {
        store: store.clone(),
        deploy_tx,
        max_concurrent_builds: std::env::var("MAX_CONCURRENT_BUILDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(constants::DEFAULT_MAX_CONCURRENT_BUILDS),
        work_dir: std::env::var("WORK_DIR").unwrap_or_else(|_| constants::DEFAULT_WORK_DIR.into()),
    };

    let port_allocator = Arc::new(PortAllocator::new());
    let deployer_config = kennel_deployer::DeployerConfig {
        store: store.clone(),
        port_allocator,
        router_tx: Some(router_update_tx.clone()),
        base_domain: std::env::var("BASE_DOMAIN")
            .unwrap_or_else(|_| constants::DEFAULT_BASE_DOMAIN.into()),
    };

    let router_config = kennel_router::RouterConfig {
        store: store.clone(),
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
    };

    let api_host = std::env::var("API_HOST").unwrap_or_else(|_| constants::DEFAULT_API_HOST.into());
    let api_port = std::env::var("API_PORT").unwrap_or_else(|_| constants::DEFAULT_API_PORT.into());
    let api_addr = format!("{api_host}:{api_port}");

    let webhook_router = kennel_webhook::router(webhook_config);
    let api_router = kennel_api::router(store.clone()).merge(webhook_router);

    // Spawn builder worker pool
    let builder_handle = tokio::spawn(kennel_builder::run_worker_pool(build_rx, builder_config));

    // Spawn deployer
    let deployer_config_clone = deployer_config.clone();
    let deployer_handle = tokio::spawn(kennel_deployer::run_deployer(
        deploy_rx,
        deployer_config_clone,
    ));

    // Spawn teardown worker
    let deployer_config_clone = deployer_config.clone();
    let teardown_handle = tokio::spawn(kennel_deployer::run_teardown_worker(
        teardown_rx,
        deployer_config_clone,
    ));

    // Spawn cleanup job
    let deployer_config_clone = deployer_config.clone();
    let teardown_tx_clone = teardown_tx.clone();
    let cleanup_handle = tokio::spawn(kennel_deployer::run_cleanup_job(
        deployer_config_clone,
        teardown_tx_clone,
    ));

    // Spawn router
    let router_store = store.clone();
    let routing_table = Arc::new(kennel_router::RoutingTable::new());
    let routing_table_clone = routing_table.clone();
    let router_handle = tokio::spawn(async move {
        if let Err(e) = kennel_router::run_router(router_config, router_update_rx).await {
            tracing::error!("Router failed: {}", e);
        }
    });

    // Spawn health monitor
    let health_handle = tokio::spawn(kennel_router::run_health_monitor(
        routing_table_clone,
        router_store,
    ));

    tracing::info!("Starting API server on {api_addr}");
    let listener = TcpListener::bind(&api_addr).await?;
    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            api_router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        {
            tracing::error!("API server failed: {}", e);
        }
    });

    tokio::select! {
        _ = tokio::time::sleep(constants::SHUTDOWN_TIMEOUT) => {
            tracing::warn!("Shutdown timeout reached, forcing exit");
        }
        _ = async {
            let _ = tokio::join!(
                server_handle,
                builder_handle,
                deployer_handle,
                teardown_handle,
                cleanup_handle,
                router_handle,
                health_handle,
            );
        } => {
            tracing::info!("All components shut down gracefully");
        }
    }

    tracing::info!("Kennel shutdown complete");

    Ok(())
}
