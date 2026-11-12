//! Opaque session tokens and the [`CurrentUser`] extractor.
//!
//! A token is 32 random bytes, base64url without padding. Only its SHA-256 hash is
//! stored, so a database leak doesn't leak usable tokens. Sessions slide: each use
//! pushes the expiry to [`SESSION_TTL`] from now, written at most once per
//! [`TOUCH_INTERVAL`] to avoid a write on every request.

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use sqlx::PgExecutor;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

pub const SESSION_TTL: Duration = Duration::days(90);
pub const TOUCH_INTERVAL: Duration = Duration::hours(1);

/// The signed-in user. Rejects with 401 when the bearer token is missing, unknown or
/// expired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
}

/// A fresh random token (what the client keeps) and its hash (what we store).
pub fn new_token() -> (String, [u8; 32]) {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).expect("OS random number generator is unavailable");
    let token = URL_SAFE_NO_PAD.encode(raw);
    let hash = hash_token(&token);
    (token, hash)
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Creates a session for `user_id` and returns its token.
pub async fn create_session(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    device_label: Option<&str>,
) -> Result<String, sqlx::Error> {
    let (token, hash) = new_token();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, device_label, expires_at) \
         VALUES ($1, $2, $3, $4, now() + $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(&hash[..])
    .bind(device_label)
    .bind(SESSION_TTL)
    .execute(db)
    .await?;
    Ok(token)
}

fn bearer(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer(parts).ok_or(ApiError::Unauthorized)?;
        let hash = hash_token(token);
        let row: Option<(Uuid, Uuid, OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, user_id, last_used_at, expires_at FROM sessions WHERE token_hash = $1",
        )
        .bind(&hash[..])
        .fetch_optional(&state.db)
        .await?;
        let (session_id, user_id, last_used_at, expires_at) = row.ok_or(ApiError::Unauthorized)?;

        let now = OffsetDateTime::now_utc();
        if expires_at <= now {
            sqlx::query("DELETE FROM sessions WHERE id = $1")
                .bind(session_id)
                .execute(&state.db)
                .await?;
            return Err(ApiError::Unauthorized);
        }
        if now - last_used_at >= TOUCH_INTERVAL {
            sqlx::query(
                "UPDATE sessions SET last_used_at = now(), expires_at = now() + $2 WHERE id = $1",
            )
            .bind(session_id)
            .bind(SESSION_TTL)
            .execute(&state.db)
            .await?;
        }
        Ok(Self {
            user_id,
            session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_base64url_and_hash_matches() {
        let (a, ha) = new_token();
        let (b, _) = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_eq!(hash_token(&a), ha);
    }

    fn parts(auth: Option<&str>) -> Parts {
        let mut b = axum::http::Request::builder();
        if let Some(v) = auth {
            b = b.header(header::AUTHORIZATION, v);
        }
        b.body(()).unwrap().into_parts().0
    }

    #[test]
    fn parses_bearer_header() {
        assert_eq!(bearer(&parts(Some("Bearer abc"))), Some("abc"));
        assert_eq!(bearer(&parts(Some("bearer abc"))), Some("abc"));
        assert_eq!(bearer(&parts(Some("Basic abc"))), None);
        assert_eq!(bearer(&parts(Some("Bearer "))), None);
        assert_eq!(bearer(&parts(None)), None);
    }
}
