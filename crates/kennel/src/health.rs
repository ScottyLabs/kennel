use kennel_config::constants::{HEALTHCHECK_PATH, HEALTHCHECK_TIMEOUT};
use reqwest::Client;
use std::sync::LazyLock;

static HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(HEALTHCHECK_TIMEOUT)
        .no_proxy()
        .build()
        .expect("failed to build healthcheck HTTP client")
});

/// Probes a service's health endpoint and returns true on HTTP 200.
pub async fn probe(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}{HEALTHCHECK_PATH}");
    matches!(HTTP.get(&url).send().await, Ok(r) if r.status().is_success())
}
