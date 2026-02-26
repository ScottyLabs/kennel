use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use std::path::Path;
use tracing::warn;

pub async fn serve_static(path: &Path, request_path: &str, spa: bool) -> Response<Body> {
    let mut file_path = path.join(request_path.trim_start_matches('/'));

    if file_path.is_dir() {
        file_path = file_path.join("index.html");
    }

    match tokio::fs::read(&file_path).await {
        Ok(contents) => {
            let mime_type = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) if spa => {
            let index_path = path.join("index.html");
            match tokio::fs::read(&index_path).await {
                Ok(contents) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(contents))
                    .unwrap(),
                Err(e) => {
                    warn!("Failed to read SPA index.html: {}", e);
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("Not found"))
                        .unwrap()
                }
            }
        }
        Err(e) => {
            warn!("Failed to read static file {:?}: {}", file_path, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not found"))
                .unwrap()
        }
    }
}
