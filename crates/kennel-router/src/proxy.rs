use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use std::net::IpAddr;
use tracing::{error, warn};

pub async fn proxy_to_service(
    request: Request<Body>,
    port: u16,
    client_ip: IpAddr,
    client: &reqwest::Client,
) -> Response<Body> {
    let uri = request.uri();
    let backend_url = format!(
        "http://127.0.0.1:{}{}",
        port,
        uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
    );

    let method = request.method().clone();
    let mut headers = request.headers().clone();

    // Add X-Forwarded-* headers
    if let Some(host) = headers.get("host") {
        headers.insert("x-forwarded-host", host.clone());
    }

    let proto = if request.uri().scheme().map(|s| s.as_str()) == Some("https") {
        "https"
    } else {
        "http"
    };
    if let Ok(val) = proto.parse() {
        headers.insert("x-forwarded-proto", val);
    }

    let forwarded_for = if let Some(existing) = headers.get("x-forwarded-for") {
        format!("{}, {}", existing.to_str().unwrap_or(""), client_ip)
    } else {
        client_ip.to_string()
    };
    if let Ok(val) = forwarded_for.parse() {
        headers.insert("x-forwarded-for", val);
    }

    let body_bytes = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Failed to read request body"))
                .unwrap();
        }
    };

    match client
        .request(method, &backend_url)
        .headers(headers)
        .body(body_bytes)
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
