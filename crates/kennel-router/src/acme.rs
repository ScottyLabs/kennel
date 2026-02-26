use rustls_acme::{AcmeConfig, caches::DirCache};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::{error, info};

pub type AcmeState = rustls_acme::AcmeState<std::io::Error, std::io::Error>;

pub fn create_acme_state(
    domains: Vec<String>,
    email: String,
    cache_dir: PathBuf,
    production: bool,
) -> AcmeState {
    info!("Initializing ACME for domains: {:?}", domains);

    if production {
        info!("Using Let's Encrypt production environment");
    } else {
        info!("Using Let's Encrypt staging environment");
    }

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let client_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );

    AcmeConfig::new_with_client_config(domains, client_config)
        .contact(vec![format!("mailto:{}", email)])
        .cache(DirCache::new(cache_dir))
        .directory_lets_encrypt(production)
        .state()
}

pub async fn run_acme_event_loop(mut state: AcmeState) {
    loop {
        match state.next().await {
            Some(Ok(event)) => info!("ACME event: {:?}", event),
            Some(Err(err)) => error!("ACME error: {:?}", err),
            None => break,
        }
    }
}
