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
    /// The account's email. Missing from servers before v0.1.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Body of `POST /api/v1/auth/google`: an ID token from the platform's Google sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleLoginRequest {
    pub id_token: String,
    /// Checked against the token's `nonce` claim when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
}

/// Body of `GET /api/v1/auth/methods`: which ways of signing in this server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AuthMethods {
    /// Google sign-in (web flow and `POST /auth/google`).
    #[serde(default)]
    pub google_sign_in: bool,
    /// `POST /auth/dev-login` (development servers only).
    #[serde(default)]
    pub dev_login: bool,
}

/// Body of `GET /api/v1/me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeResponse {
    pub user_id: Uuid,
}
