//! Sign-in and session types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Body of `POST /api/v1/auth/dev-login` (development servers only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevLoginRequest {
    pub email: String,
    /// Shown in a future "signed-in devices" list, e.g. `"iPhone"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
}

/// A new session. Send `token` as `Authorization: Bearer <token>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: Uuid,
}

/// Body of `GET /api/v1/me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeResponse {
    pub user_id: Uuid,
}
