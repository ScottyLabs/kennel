use axum::{
    extract::{Query, State},
    http::StatusCode,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::AppState;

pub async fn check_domain(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> StatusCode {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    let known = matches!(
        state.store.deployments().find_by_domain(domain).await,
        Ok(Some(_))
    );
    if !known {
        return StatusCode::NOT_FOUND;
    }

    // Refuse issuance until the domain resolves to kennel's public IP, since
    // repeated failed ACME orders get the Let's Encrypt account paused
    if let Some(cf) = &state.cloudflare {
        if !resolves_to(domain, cf.public_ip()).await {
            tracing::warn!(domain = %domain, "refusing certificate, domain does not resolve to kennel");
            return StatusCode::NOT_FOUND;
        }
    }

    StatusCode::OK
}

async fn resolves_to(domain: &str, public_ip: &str) -> bool {
    let Ok(expected) = public_ip.parse::<IpAddr>() else {
        return false;
    };

    let lookup = tokio::net::lookup_host((domain, 443));
    let Ok(Ok(addrs)) = tokio::time::timeout(Duration::from_secs(3), lookup).await else {
        return false;
    };

    addrs.map(|a| a.ip()).any(|ip| ip == expected)
}
