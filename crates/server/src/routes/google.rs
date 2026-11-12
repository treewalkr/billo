//! Google sign-in for the web app: authorization code flow with PKCE (S256), `state`
//! and `nonce`.
//!
//! 1. `GET /api/v1/auth/google/start?return_to=<app origin>` stores a 10-minute,
//!    single-use login attempt and redirects to Google.
//! 2. `GET /api/v1/auth/google/callback?code&state` consumes the attempt, exchanges the
//!    code, verifies the ID token, signs the user in and redirects back to the app with
//!    the session in the URL fragment (`#billo_token=…&user_id=…&email=…`), which
//!    browsers never send to servers. Failures redirect to `#billo_error=<code>`.

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use billo_api::{ErrorBody, ErrorCode, ErrorDetail};
use serde::Deserialize;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::{create_session, hash_token};
use crate::extract::{Json, Query};
use crate::google::{Google, GoogleUser, VerifyError, code_challenge, random_string};

/// How long a started login stays valid.
pub const ATTEMPT_TTL: Duration = Duration::minutes(10);

#[derive(Debug, Deserialize)]
pub struct StartQuery {
    return_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `billo_error` codes the app translates.
mod code {
    pub const CANCELLED: &str = "cancelled";
    pub const EXPIRED: &str = "expired";
    pub const GOOGLE: &str = "google";
    pub const INVALID_TOKEN: &str = "invalid_token";
    pub const EMAIL_UNVERIFIED: &str = "email_unverified";
    pub const SERVER: &str = "server";
}

fn google(state: &AppState) -> &Google {
    state
        .google
        .as_deref()
        .expect("Google routes are only registered when Google is configured")
}

/// `raw` if it is an http(s) URL on one of the allowed app origins, without fragment.
fn allowed_return_to(g: &Google, raw: &str) -> Option<String> {
    let mut u = url::Url::parse(raw).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some()
    {
        return None;
    }
    let origin = u.origin().ascii_serialization();
    if !g.config.app_origins.contains(&origin) {
        return None;
    }
    u.set_fragment(None);
    Some(u.into())
}

fn bad_request(message: &str) -> Response {
    let body = ErrorBody {
        error: ErrorDetail {
            code: ErrorCode::Validation,
            message: message.to_owned(),
        },
    };
    (StatusCode::BAD_REQUEST, axum::Json(body)).into_response()
}

/// 302 that nobody caches and that doesn't leak the URL as a referrer.
fn found(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, location),
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
    )
        .into_response()
}

/// `return_to` with `pairs` as its fragment.
fn with_fragment(return_to: &str, pairs: &[(&str, &str)]) -> String {
    let mut u = url::Url::parse(return_to).expect("return_to was validated at start");
    let mut f = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        f.append_pair(k, v);
    }
    u.set_fragment(Some(&f.finish()));
    u.into()
}

fn back_with_error(return_to: &str, error: &str) -> Response {
    found(&with_fragment(return_to, &[("billo_error", error)]))
}

/// For a callback we can't tie to a started login: there is nowhere safe to send the
/// user back to.
fn error_page() -> Response {
    let html = "<!doctype html><meta charset=utf-8><title>Billo</title>\
        <p>This sign-in link has expired or was already used. \
        Go back to Billo and sign in again.</p>\
        <p lang=th>ลิงก์เข้าสู่ระบบนี้หมดอายุหรือถูกใช้ไปแล้ว \
        กรุณากลับไปที่ Billo แล้วเข้าสู่ระบบอีกครั้ง</p>";
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

/// `GET /auth/google/start`.
pub async fn start(State(state): State<AppState>, Query(q): Query<StartQuery>) -> Response {
    let g = google(&state);
    let Some(return_to) = q.return_to.as_deref().and_then(|r| allowed_return_to(g, r)) else {
        return bad_request("return_to is missing or not an allowed app origin");
    };
    let login_state = random_string();
    let verifier = random_string();
    let nonce = random_string();
    // Old attempts are cleaned up lazily, well after they've expired.
    let cleanup =
        sqlx::query("DELETE FROM login_attempts WHERE expires_at < now() - interval '1 day'")
            .execute(&state.db)
            .await;
    let insert = sqlx::query(
        "INSERT INTO login_attempts (state_hash, pkce_verifier, nonce, return_to, expires_at) \
         VALUES ($1, $2, $3, $4, now() + $5)",
    )
    .bind(&hash_token(&login_state)[..])
    .bind(&verifier)
    .bind(&nonce)
    .bind(&return_to)
    .bind(ATTEMPT_TTL)
    .execute(&state.db)
    .await;
    if let Err(e) = cleanup.and(insert) {
        tracing::error!(error = %e, "google start: cannot store login attempt");
        return back_with_error(&return_to, code::SERVER);
    }
    found(&g.authorize_url(&login_state, &nonce, &code_challenge(&verifier)))
}

/// `GET /auth/google/callback`.
pub async fn callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    let g = google(&state);
    let Some(login_state) = q.state.as_deref().filter(|s| !s.is_empty()) else {
        return error_page();
    };
    // Single use: the row is gone after this, whatever happens next.
    let row: Result<Option<(String, String, String, OffsetDateTime)>, _> = sqlx::query_as(
        "DELETE FROM login_attempts WHERE state_hash = $1 \
         RETURNING pkce_verifier, nonce, return_to, expires_at",
    )
    .bind(&hash_token(login_state)[..])
    .fetch_optional(&state.db)
    .await;
    let (verifier, nonce, return_to, expires_at) = match row {
        Ok(Some(r)) => r,
        Ok(None) => {
            tracing::warn!("google callback: unknown or already used state");
            return error_page();
        }
        Err(e) => {
            tracing::error!(error = %e, "google callback: cannot read login attempt");
            return error_page();
        }
    };
    if expires_at <= OffsetDateTime::now_utc() {
        return back_with_error(&return_to, code::EXPIRED);
    }
    if let Some(err) = q.error.as_deref() {
        tracing::info!(error = err, "google callback: Google returned an error");
        let code = if err == "access_denied" {
            code::CANCELLED
        } else {
            code::GOOGLE
        };
        return back_with_error(&return_to, code);
    }
    let Some(auth_code) = q.code.as_deref().filter(|c| !c.is_empty()) else {
        return back_with_error(&return_to, code::GOOGLE);
    };
    let id_token = match g.exchange_code(auth_code, &verifier).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "google callback: code exchange failed");
            return back_with_error(&return_to, code::GOOGLE);
        }
    };
    let user = match g
        .verify(&id_token, &[g.config.client_id.as_str()], Some(&nonce))
        .await
    {
        Ok(u) => u,
        Err(e) => return back_with_error(&return_to, verify_error_code(&e)),
    };
    match sign_in(&state.db, &user, Some("web")).await {
        Ok((token, user_id)) => found(&with_fragment(
            &return_to,
            &[
                ("billo_token", &token),
                ("user_id", &user_id.to_string()),
                ("email", &user.email),
            ],
        )),
        Err(e) => {
            tracing::error!(error = %e, "google callback: cannot create session");
            back_with_error(&return_to, code::SERVER)
        }
    }
}

fn verify_error_code(e: &VerifyError) -> &'static str {
    match e {
        VerifyError::Invalid(reason) => {
            tracing::warn!(reason, "google id token rejected");
            code::INVALID_TOKEN
        }
        VerifyError::EmailUnverified => {
            tracing::warn!("google id token rejected: email not verified");
            code::EMAIL_UNVERIFIED
        }
        VerifyError::Keys(e) => {
            tracing::error!(error = %e, "cannot fetch Google's signing keys");
            code::GOOGLE
        }
    }
}

/// Finds or creates the user by Google `sub` (keeping the email current) and starts a
/// session. Returns `(token, user_id)`.
pub async fn sign_in(
    db: &PgPool,
    user: &GoogleUser,
    device_label: Option<&str>,
) -> Result<(String, Uuid), sqlx::Error> {
    let mut tx = db.begin().await?;
    let (user_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO users (id, google_sub, email) VALUES ($1, $2, $3) \
         ON CONFLICT (google_sub) DO UPDATE SET email = EXCLUDED.email \
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(&user.sub)
    .bind(&user.email)
    .fetch_one(&mut *tx)
    .await?;
    let token = create_session(&mut *tx, user_id, device_label).await?;
    tx.commit().await?;
    Ok((token, user_id))
}

/// `GET /auth/methods`: which sign-in methods this server offers.
pub async fn methods(State(state): State<AppState>) -> Json<billo_api::AuthMethods> {
    Json(billo_api::AuthMethods {
        google_sign_in: state.google.is_some(),
        dev_login: state.dev_login,
    })
}
