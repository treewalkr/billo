//! Talking to the Billo server: sign-in and sync. Screens never call the server
//! directly; they use the local store (`store`), which the sync engine keeps in step.

use std::fmt;

use billo_api::sync::{PullResponse, PushRequest, PushResponse};
use billo_api::{
    DevLoginRequest, ErrorBody, ErrorCode, ErrorDetail, HealthResponse, LoginResponse,
};
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

/// What users see: plain words in their language. The server's own message is kept
/// for [`ApiError::details`].
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(_) => f.write_str(s::err_network()),
            Self::Unauthorized => f.write_str(s::error_code_text(ErrorCode::Unauthorized)),
            Self::NotFound => f.write_str(s::error_code_text(ErrorCode::NotFound)),
            Self::Validation(m) => {
                let head = s::error_code_text(ErrorCode::Validation);
                match s::validation_text(m) {
                    // A whole sentence ("This email address isn't valid.") stands alone.
                    Some(text) if !m.contains(": ") => f.write_str(&text),
                    Some(text) => write!(f, "{head}: {text}"),
                    None => f.write_str(s::err_check_input()),
                }
            }
            Self::Conflict(_) => f.write_str(s::error_code_text(ErrorCode::Conflict)),
            Self::Server(..) => f.write_str(s::error_code_text(ErrorCode::Internal)),
            Self::Decode(_) => f.write_str(s::err_decode()),
        }
    }
}

impl ApiError {
    /// The raw error, for the "Details" line: the server's code and message, the HTTP
    /// status, or the network / decoding error. In English, for debugging.
    pub fn details(&self) -> Option<String> {
        Some(match self {
            Self::Network(m) => format!("network: {m}"),
            Self::Unauthorized => "unauthorized".to_owned(),
            Self::NotFound => "not_found".to_owned(),
            Self::Validation(m) => format!("validation: {m}"),
            Self::Conflict(m) => format!("conflict: {m}"),
            Self::Server(status, m) if m.is_empty() => format!("HTTP {status}"),
            Self::Server(status, m) => format!("HTTP {status}: {m}"),
            Self::Decode(m) => format!("decode: {m}"),
        })
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

/// A rejected sync mutation's error as an [`ApiError`], for showing it.
pub fn error_from_detail(detail: &ErrorDetail) -> ApiError {
    let m = detail.message.clone();
    match detail.code {
        ErrorCode::NotFound => ApiError::NotFound,
        ErrorCode::Validation => ApiError::Validation(m),
        ErrorCode::Unauthorized => ApiError::Unauthorized,
        ErrorCode::Conflict => ApiError::Conflict(m),
        ErrorCode::Internal => ApiError::Server(500, m),
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
    fn every_server_code_maps_to_translated_text() {
        use crate::strings::{self as s, Lang};
        let body =
            |code: &str, msg: &str| format!(r#"{{"error":{{"code":"{code}","message":"{msg}"}}}}"#);
        let cases = [
            ("not_found", 404, ErrorCode::NotFound),
            ("validation", 422, ErrorCode::Validation),
            ("unauthorized", 401, ErrorCode::Unauthorized),
            ("conflict", 409, ErrorCode::Conflict),
            ("internal", 500, ErrorCode::Internal),
        ];
        for l in Lang::ALL {
            s::set_lang(l);
            for (name, status, code) in cases {
                // The wire name is the code's serde name.
                assert_eq!(
                    serde_json::from_str::<ErrorCode>(&format!("\"{name}\"")).unwrap(),
                    code
                );
                let e = error_from(status, &body(name, "raw message"));
                let shown = e.to_string();
                let expected = if code == ErrorCode::Validation {
                    s::err_check_input()
                } else {
                    s::error_code_text(code)
                };
                assert!(shown.starts_with(expected), "{name} in {l:?}: {shown}");
                assert!(!shown.contains("raw message"), "{name}: raw text leaked");
                let details = e.details().unwrap();
                assert!(
                    details.contains(name) || details.contains("HTTP 500"),
                    "{details}"
                );
                if code != ErrorCode::Unauthorized && code != ErrorCode::NotFound {
                    assert!(details.contains("raw message"), "{name}: {details}");
                }
            }
        }
        s::set_lang(Lang::En);
    }

    #[test]
    fn validation_errors_show_the_translated_problem() {
        use crate::strings::{self as s, Lang};
        s::set_lang(Lang::Th);
        let e = ApiError::Validation("amount: must be positive".into());
        assert_eq!(e.to_string(), "เซิร์ฟเวอร์ไม่รับข้อมูลนี้: จำนวนเงิน: ต้องมากกว่าศูนย์");
        assert_eq!(
            e.details().as_deref(),
            Some("validation: amount: must be positive")
        );
        let unknown = ApiError::Validation("payload: expected a string".into());
        assert_eq!(
            unknown.to_string(),
            "เซิร์ฟเวอร์ไม่รับข้อมูลนี้ กรุณาตรวจสอบข้อมูลที่กรอก"
        );
        let sentence = ApiError::Validation("email is not a valid address".into());
        assert_eq!(sentence.to_string(), "อีเมลนี้ไม่ถูกต้อง");
        s::set_lang(Lang::En);
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
