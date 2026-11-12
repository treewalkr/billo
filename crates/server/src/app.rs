//! Router construction, separate from `main` so tests can drive it directly.

use axum::Router;
use axum::routing::get;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::routes;

#[derive(Debug, Clone)]
pub struct AppState {
    pub db: PgPool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(routes::health::health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
