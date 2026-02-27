mod channels;
mod config;
mod dns;
mod reconcile;
mod signal;

use kennel_config::constants;
use kennel_store::Store;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    tracing::info!("Database migrations complete");

    // Reconcile projects from NixOS configuration
    if let Err(e) = reconcile::reconcile_projects(store.clone()).await {
        tracing::error!("Project reconciliation failed: {}", e);
        return Err(e);
    }

    // Reconcile deployments and resources on startup
    if let Err(e) = reconcile::reconcile_deployments(store.clone()).await {
        tracing::error!("Startup reconciliation failed: {}", e);
    }

    let channels = channels::create_channels();
    let base_domain =
        std::env::var("BASE_DOMAIN").unwrap_or_else(|_| constants::DEFAULT_BASE_DOMAIN.into());

    let dns_manager = dns::initialize_dns(store.clone(), &base_domain).await?;
    let builder_config = config::create_builder_config(store.clone(), channels.deploy_tx.clone());
    let deployer_config = config::create_deployer_config(
        store.clone(),
        channels.router_update_tx.clone(),
        dns_manager,
        base_domain,
    );
    let router_config = config::create_router_config(store.clone());

    let webhook_config = kennel_webhook::WebhookConfig {
        store: store.clone(),
        build_tx: channels.build_tx,
        teardown_tx: channels.teardown_tx.clone(),
    };

    let api_host = std::env::var("API_HOST").unwrap_or_else(|_| constants::DEFAULT_API_HOST.into());
    let api_port = std::env::var("API_PORT").unwrap_or_else(|_| constants::DEFAULT_API_PORT.into());
    let api_addr = format!("{api_host}:{api_port}");

    let webhook_router = kennel_webhook::router(webhook_config);
    let api_router = kennel_api::router(store.clone()).merge(webhook_router);

    // Spawn builder worker pool
    let builder_handle = tokio::spawn(kennel_builder::run_worker_pool(
        channels.build_rx,
        builder_config,
    ));

    // Spawn deployer
    let deployer_handle = tokio::spawn(kennel_deployer::run_deployer(
        channels.deploy_rx,
        deployer_config.clone(),
    ));

    // Spawn teardown worker
    let teardown_handle = tokio::spawn(kennel_deployer::run_teardown_worker(
        channels.teardown_rx,
        deployer_config.clone(),
    ));

    // Spawn cleanup job
    let cleanup_handle = tokio::spawn(kennel_deployer::run_cleanup_job(
        deployer_config.clone(),
        channels.teardown_tx.clone(),
    ));

    // Spawn build log cleanup job
    let log_cleanup_handle = tokio::spawn(kennel_deployer::run_log_cleanup_job(
        deployer_config.clone(),
    ));

    // Spawn router
    let router_store = store.clone();
    let routing_table = Arc::new(kennel_router::RoutingTable::new());
    let routing_table_clone = routing_table.clone();
    let router_handle = tokio::spawn(async move {
        if let Err(e) = kennel_router::run_router(router_config, channels.router_update_rx).await {
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
        .with_graceful_shutdown(signal::shutdown_signal())
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
                log_cleanup_handle,
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
