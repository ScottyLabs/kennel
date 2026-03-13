mod channels;
mod config;
mod dns;
mod reconcile;
mod signal;

use kennel_config::constants;
use kennel_store::Store;
use kennel_supervisor::{Supervisor, SupervisorEvent};
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
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

    migration::Migrator::up(&db, None).await?;

    let store = Arc::new(Store::new(db));

    tracing::info!("Database migrations complete");

    if let Err(e) = reconcile::reconcile_projects(store.clone()).await {
        tracing::error!("Project reconciliation failed: {}", e);
        return Err(e);
    }

    // Initialize supervisor with event channel.
    let (event_tx, _) =
        tokio::sync::broadcast::channel::<SupervisorEvent>(constants::SUPERVISOR_EVENT_CAPACITY);
    let supervisor = Arc::new(Mutex::new(Supervisor::new(event_tx.clone())));

    // Reconcile deployments by re-starting processes through the supervisor.
    if let Err(e) = reconcile::reconcile_deployments(store.clone(), supervisor.clone()).await {
        tracing::error!("Startup reconciliation failed: {}", e);
    }

    let channels = channels::create_channels();
    let base_domain =
        std::env::var("BASE_DOMAIN").unwrap_or_else(|_| constants::DEFAULT_BASE_DOMAIN.into());

    let dns_manager = dns::initialize_dns(store.clone(), &base_domain).await?;
    let builder_config = config::create_builder_config(store.clone(), channels.deploy_tx.clone());
    let mut resource_providers: Vec<Arc<dyn kennel_provision::ResourceProvider>> = vec![];

    if let Ok(socket_dir) = std::env::var("POSTGRES_SOCKET_DIR") {
        resource_providers.push(Arc::new(kennel_provision::postgres::PostgresProvider::new(
            store.db().clone(),
            socket_dir,
        )));
    }

    if let Ok(socket_path) = std::env::var("VALKEY_SOCKET_PATH") {
        resource_providers.push(Arc::new(kennel_provision::valkey::ValkeyProvider::new(
            socket_path,
        )));
    }

    if let Ok(admin_endpoint) = std::env::var("GARAGE_ADMIN_ENDPOINT") {
        let s3_endpoint =
            std::env::var("GARAGE_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:3900".into());
        let admin_token = std::env::var("GARAGE_ADMIN_TOKEN")
            .expect("GARAGE_ADMIN_TOKEN required when GARAGE_ADMIN_ENDPOINT is set");
        resource_providers.push(Arc::new(kennel_provision::garage::GarageProvider::new(
            admin_endpoint,
            s3_endpoint,
            admin_token,
        )));
    }

    let deployer_config = config::create_deployer_config(
        store.clone(),
        supervisor.clone(),
        dns_manager,
        resource_providers,
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

    let builder_handle = tokio::spawn(kennel_builder::run_worker_pool(
        channels.build_rx,
        builder_config,
    ));

    let deployer_handle = tokio::spawn(kennel_deployer::run_deployer(
        channels.deploy_rx,
        deployer_config.clone(),
    ));

    let teardown_handle = tokio::spawn(kennel_deployer::run_teardown_worker(
        channels.teardown_rx,
        deployer_config.clone(),
    ));

    let cleanup_handle = tokio::spawn(kennel_deployer::run_cleanup_job(
        deployer_config.clone(),
        channels.teardown_tx.clone(),
    ));

    let log_cleanup_handle = tokio::spawn(kennel_deployer::run_log_cleanup_job(
        deployer_config.clone(),
    ));

    // Router subscribes to supervisor events for routing table updates.
    let event_rx = event_tx.subscribe();
    let router_handle = tokio::spawn(async move {
        if let Err(e) = kennel_router::run_router(router_config, event_rx).await {
            tracing::error!("Router failed: {}", e);
        }
    });

    // Signal systemd that startup is complete (migrations, reconciliation,
    // and all worker tasks are running).
    let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);

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
            );
        } => {
            tracing::info!("All components shut down gracefully");
        }
    }

    tracing::info!("Kennel shutdown complete");

    Ok(())
}
