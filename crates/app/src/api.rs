//! Talking to the Billo server.

use billo_api::HealthResponse;

/// Server base URL, set at build time with `BILLO_API_URL`.
pub const BASE_URL: &str = match option_env!("BILLO_API_URL") {
    Some(url) => url,
    None => "http://localhost:8080",
};

/// `GET /api/v1/health`.
pub async fn health() -> Result<HealthResponse, reqwest::Error> {
    reqwest::get(format!("{BASE_URL}/api/v1/health"))
        .await?
        .error_for_status()?
        .json()
        .await
}
