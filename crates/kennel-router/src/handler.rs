use crate::table::{RouteTarget, RoutingTable};
use crate::{proxy, static_serve};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Response, StatusCode};
use axum_extra::TypedHeader;
use axum_extra::headers::Host;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

pub async fn route_request(
    State(table): State<Arc<RoutingTable>>,
    TypedHeader(host): TypedHeader<Host>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response<Body> {
    let domain = host.hostname();

    info!("Routing request for domain: {} from {}", domain, addr);

    match table.get(domain).await {
        Some(route) => match route.target {
            RouteTarget::Service { port } => {
                info!("Proxying to service on port {}", port);
                proxy::proxy_to_service(request, port, addr.ip()).await
            }
            RouteTarget::StaticSite { path, spa } => {
                info!("Serving static site from {:?}", path);
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
