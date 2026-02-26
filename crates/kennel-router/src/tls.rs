use crate::acme::{AcmeState, run_acme_event_loop};
use axum::Router;
use std::net::SocketAddr;
use tracing::info;

pub async fn serve_with_tls(
    router: Router,
    addr: SocketAddr,
    state: AcmeState,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting HTTPS server on {}", addr);

    let acceptor = state.axum_acceptor(state.default_rustls_config());

    tokio::spawn(async move {
        run_acme_event_loop(state).await;
    });

    axum_server::bind(addr)
        .acceptor(acceptor)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}
