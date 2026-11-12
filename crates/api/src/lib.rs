//! Billo API: request/response types shared by the server and the app.
//!
//! Everything here is part of the `/api/v1` contract and must stay backward-compatible.

use serde::{Deserialize, Serialize};

pub mod auth;
pub mod error;

pub use auth::{DevLoginRequest, LoginResponse, MeResponse};
pub use error::{ErrorBody, ErrorCode, ErrorDetail};

/// Body of `GET /api/v1/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// `"ok"`, or `"degraded"` when a dependency (e.g. the database) is down.
    pub status: String,
    /// Workspace version, `X.Y.Z`.
    pub version: String,
    /// Build number; only ever goes up.
    pub build: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_json_shape() {
        let h = HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
            build: 1,
        };
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, r#"{"status":"ok","version":"0.1.0","build":1}"#);
        assert_eq!(serde_json::from_str::<HealthResponse>(&json).unwrap(), h);
    }
}
