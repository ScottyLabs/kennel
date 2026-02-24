use axum::Router;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(info(
    title = "Kennel API",
    description = "Branch-based deployment platform",
    license(name = "AGPL-3.0-or-later"),
))]
struct ApiDoc;

#[utoipa::path(get, path = "/health", responses((status = OK, body = str)))]
async fn health() -> &'static str {
    "ok"
}

pub fn router() -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(utoipa_axum::routes!(health))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", api))
        .layer(TraceLayer::new_for_http())
}
