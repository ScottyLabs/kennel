use axum::Router;
use kennel_store::Store;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
    ),
    info(
        title = "Kennel API",
        version = "0.1.0",
        description = "Branch-based deployment platform powered by Nix",
        license(name = "AGPL-3.0-or-later"),
    )
)]
struct ApiDoc;

#[utoipa::path(get, path = "/health", responses((status = OK, body = str)))]
async fn health(
    axum::extract::State(store): axum::extract::State<Arc<Store>>,
) -> (axum::http::StatusCode, &'static str) {
    match store.db().ping().await {
        Ok(()) => (axum::http::StatusCode::OK, "ok"),
        Err(_) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "database unreachable",
        ),
    }
}

pub fn router(store: Arc<Store>) -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(utoipa_axum::routes!(health))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .with_state(store)
}
