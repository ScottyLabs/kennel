use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use std::path::Path;
use tracing::warn;

async fn serve_spa_fallback(index_path: &Path) -> Response<Body> {
    match tokio::fs::read(index_path).await {
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

pub async fn serve_static(path: &Path, request_path: &str, spa: bool) -> Response<Body> {
    let mut file_path = path.join(request_path.trim_start_matches('/'));

    let base_canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to canonicalize base path {:?}: {}", path, e);
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap();
        }
    };

    let file_canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            if spa {
                let index_path = path.join("index.html");
                return serve_spa_fallback(&index_path).await;
            }
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not found"))
                .unwrap();
        }
    };

    if !file_canonical.starts_with(&base_canonical) {
        warn!(
            "Path traversal attempt: {:?} not within {:?}",
            file_canonical, base_canonical
        );
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Forbidden"))
            .unwrap();
    }

    if file_canonical.is_dir() {
        file_path = file_canonical.join("index.html");
    } else {
        file_path = file_canonical;
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
        Err(_) if spa => serve_spa_fallback(&path.join("index.html")).await,
        Err(e) => {
            warn!("Failed to read static file {:?}: {}", file_path, e);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not found"))
                .unwrap()
        }
    }
}
