use crate::table::{RouteTarget, RoutingTable};
use crate::{proxy, static_serve};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Response, StatusCode};
use axum_extra::TypedHeader;
use axum_extra::headers::Host;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct RouterState {
    pub table: Arc<RoutingTable>,
    pub http_client: reqwest::Client,
}

pub async fn route_request(
    State(state): State<RouterState>,
    TypedHeader(host): TypedHeader<Host>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response<Body> {
    let domain = host.hostname();

    debug!("Routing request for domain: {} from {}", domain, addr);

    match state.table.get(domain).await {
        Some(route) => match route.target {
            RouteTarget::Service { port } => {
                proxy::proxy_to_service(request, port, addr.ip(), &state.http_client).await
            }
            RouteTarget::StaticSite { path, spa } => {
                let request_path = request.uri().path();
                static_serve::serve_static(&path, request_path, spa).await
            }
        },
        None => {
            warn!("No route found for domain: {}", domain);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("No deployment found for this domain"))
                .unwrap()
        }
    }
}
