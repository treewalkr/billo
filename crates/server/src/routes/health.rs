//! `GET /api/v1/health`: version, build number and database reachability.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use billo_api::HealthResponse;

use crate::app::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build number from the root `Cargo.toml`, injected by `build.rs`.
pub fn build_number() -> u32 {
    env!("BILLO_BUILD")
        .parse()
        .expect("BILLO_BUILD is set by build.rs to a u32")
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let (code, status) = match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, "ok"),
        Err(e) => {
            tracing::warn!(error = %e, "health check: database unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "degraded")
        }
    };
    (
        code,
        Json(HealthResponse {
            status: status.to_owned(),
            version: VERSION.to_owned(),
            build: build_number(),
        }),
    )
}
