use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use std::net::IpAddr;
use tracing::{error, warn};

pub async fn proxy_to_service(
    request: Request<Body>,
    port: u16,
    client_ip: IpAddr,
) -> Response<Body> {
    let uri = request.uri();
    let backend_url = format!(
        "http://127.0.0.1:{}{}",
        port,
        uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
    );

    let client = reqwest::Client::new();

    let method = request.method().clone();
    let mut headers = request.headers().clone();

    // Add X-Forwarded-* headers
    if let Some(host) = headers.get("host") {
        headers.insert("x-forwarded-host", host.clone());
    }

    // Determine protocol from request
    let proto = if request.uri().scheme().map(|s| s.as_str()) == Some("https") {
        "https"
    } else {
        "http"
    };
    headers.insert("x-forwarded-proto", proto.parse().unwrap());

    // Add X-Forwarded-For header
    let forwarded_for = if let Some(existing) = headers.get("x-forwarded-for") {
        format!("{}, {}", existing.to_str().unwrap_or(""), client_ip)
    } else {
        client_ip.to_string()
    };
    headers.insert("x-forwarded-for", forwarded_for.parse().unwrap());

    match client
        .request(method.clone(), &backend_url)
        .headers(headers)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            let headers = response.headers().clone();
            let body = match response.bytes().await {
                Ok(bytes) => Body::from(bytes),
                Err(e) => {
                    error!("Failed to read backend response body: {}", e);
                    return Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from("Backend error"))
                        .unwrap();
                }
            };

            let mut builder = Response::builder().status(status);
            for (key, value) in headers.iter() {
                builder = builder.header(key, value);
            }

            builder.body(body).unwrap()
        }
        Err(e) => {
            warn!("Failed to proxy to service on port {}: {}", port, e);

            let status = if e.is_connect() || e.is_timeout() {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_GATEWAY
            };

            Response::builder()
                .status(status)
                .body(Body::from("Service unavailable"))
                .unwrap()
        }
    }
}
