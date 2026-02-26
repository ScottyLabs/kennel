mod builds;

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
        builds::cancel_build,
    ),
    tags(
        (name = "builds", description = "Build management endpoints"),
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
async fn health() -> &'static str {
    "ok"
}

pub fn router(store: Arc<Store>) -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(utoipa_axum::routes!(health))
        .routes(utoipa_axum::routes!(builds::cancel_build))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", api))
        .layer(TraceLayer::new_for_http())
        .with_state(store)
}
