//! Talking to the Billo server: sign-in and sync. Screens never call the server
//! directly; they use the local store (`store`), which the sync engine keeps in step.

use std::fmt;

use billo_api::sync::{PullResponse, PushRequest, PushResponse};
use billo_api::{DevLoginRequest, ErrorBody, ErrorCode, HealthResponse, LoginResponse};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::strings as s;

/// Server base URL, set at build time with `BILLO_API_URL`.
pub const BASE_URL: &str = match option_env!("BILLO_API_URL") {
    Some(url) => url,
    None => "http://localhost:8080",
};

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// The server couldn't be reached.
    Network(String),
    /// Missing, invalid or expired session: the app signs out.
    Unauthorized,
    NotFound,
    /// The server rejected the input (422); the message names the field.
    Validation(String),
    Conflict(String),
    /// Any other failure, with the HTTP status.
    Server(u16, String),
    /// The response wasn't what this app version expects.
    Decode(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(m) => write!(f, "{} ({m})", s::err_network()),
            Self::Unauthorized => f.write_str(s::err_session_ended()),
            Self::NotFound => f.write_str(s::err_not_found()),
            Self::Validation(m) => write!(f, "{}: {m}", s::err_not_saved()),
            Self::Conflict(m) => write!(f, "{}: {m}", s::err_conflict()),
            Self::Server(status, m) => write!(f, "{} {status}: {m}", s::err_server()),
            Self::Decode(m) => write!(f, "{} ({m})", s::err_decode()),
        }
    }
}

impl std::error::Error for ApiError {}

/// Maps a failed response's status and body to an [`ApiError`].
pub fn error_from(status: u16, body: &str) -> ApiError {
    let parsed = serde_json::from_str::<ErrorBody>(body).ok();
    let message = parsed
        .as_ref()
        .map_or_else(|| body.trim().to_owned(), |b| b.error.message.clone());
    match (status, parsed.map(|b| b.error.code)) {
        (401, _) | (_, Some(ErrorCode::Unauthorized)) => ApiError::Unauthorized,
        (404, _) | (_, Some(ErrorCode::NotFound)) => ApiError::NotFound,
        (422, _) | (_, Some(ErrorCode::Validation)) => ApiError::Validation(message),
        (409, _) | (_, Some(ErrorCode::Conflict)) => ApiError::Conflict(message),
        _ => ApiError::Server(status, message),
    }
}

/// A new client-generated id (UUIDv7, time-ordered).
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// A connection to the server, optionally signed in.
#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base: base.into(),
            token,
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    fn authed(&self, req: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    async fn send(&self, req: RequestBuilder) -> Result<Response, ApiError> {
        let res = self
            .authed(req)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let body = res.text().await.unwrap_or_default();
        Err(error_from(status.as_u16(), &body))
    }

    async fn json<T: DeserializeOwned>(&self, req: RequestBuilder) -> Result<T, ApiError> {
        let res = self.send(req).await?;
        let text = res
            .text()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))
    }

    async fn empty(&self, req: RequestBuilder) -> Result<(), ApiError> {
        let res = self.send(req).await?;
        debug_assert!(res.status() == StatusCode::NO_CONTENT || res.status().is_success());
        Ok(())
    }

    pub async fn health(&self) -> Result<HealthResponse, ApiError> {
        self.json(self.http.get(self.url("/health"))).await
    }

    /// `POST /auth/dev-login`. A 404 means the server has dev login turned off.
    pub async fn dev_login(&self, email: &str) -> Result<LoginResponse, ApiError> {
        let body = DevLoginRequest {
            email: email.to_owned(),
            device_label: None,
        };
        self.json(self.http.post(self.url("/auth/dev-login")).json(&body))
            .await
    }

    pub async fn logout(&self) -> Result<(), ApiError> {
        self.empty(self.http.post(self.url("/auth/logout"))).await
    }

    /// `POST /sync/push`.
    pub async fn sync_push(&self, req: &PushRequest) -> Result<PushResponse, ApiError> {
        self.json(self.http.post(self.url("/sync/push")).json(req))
            .await
    }

    /// `GET /sync/pull`.
    pub async fn sync_pull(&self, since: i64, limit: u32) -> Result<PullResponse, ApiError> {
        self.json(
            self.http
                .get(self.url(&format!("/sync/pull?since={since}&limit={limit}"))),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_bodies() {
        let body =
            |code: &str, msg: &str| format!(r#"{{"error":{{"code":"{code}","message":"{msg}"}}}}"#);
        assert_eq!(
            error_from(422, &body("validation", "name: required")),
            ApiError::Validation("name: required".into())
        );
        assert_eq!(
            error_from(401, &body("unauthorized", "x")),
            ApiError::Unauthorized
        );
        assert_eq!(error_from(404, &body("not_found", "x")), ApiError::NotFound);
        assert_eq!(
            error_from(409, &body("conflict", "duplicate")),
            ApiError::Conflict("duplicate".into())
        );
        assert_eq!(
            error_from(500, &body("internal", "internal error")),
            ApiError::Server(500, "internal error".into())
        );
    }

    #[test]
    fn maps_non_json_errors_by_status() {
        assert_eq!(error_from(401, ""), ApiError::Unauthorized);
        assert_eq!(error_from(404, "Not Found"), ApiError::NotFound);
        assert_eq!(
            error_from(502, "Bad Gateway\n"),
            ApiError::Server(502, "Bad Gateway".into())
        );
    }

    #[test]
    fn new_ids_are_v7_and_ordered() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.get_version_num(), 7);
        assert!(a < b);
    }
}
