//! `POST /api/v1/auth/dev-login` (only when `DEV_LOGIN=true`) and `POST /api/v1/auth/logout`.

use axum::extract::State;
use axum::http::StatusCode;
use billo_api::{DevLoginRequest, LoginResponse, MeResponse};
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::{CurrentUser, create_session};
use crate::error::{ApiError, ApiResult};
use crate::extract::Json;

/// Signs in as `email` without any proof of identity. Development only: the route is
/// not registered unless `DEV_LOGIN=true`. The user is keyed by `google_sub = "dev:<email>"`,
/// so dev users never collide with real Google accounts.
pub async fn dev_login(
    State(state): State<AppState>,
    Json(req): Json<DevLoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    let email = req.email.trim().to_lowercase();
    if email.len() > 254 || !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Err(ApiError::validation("email is not a valid address"));
    }
    let device_label = req.device_label.as_deref().map(str::trim);
    if device_label.is_some_and(|l| l.chars().count() > 100) {
        return Err(ApiError::validation("device_label is too long"));
    }

    let mut tx = state.db.begin().await?;
    let (user_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO users (id, google_sub, email) VALUES ($1, $2, $3) \
         ON CONFLICT (google_sub) DO UPDATE SET email = EXCLUDED.email \
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(format!("dev:{email}"))
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;
    let token = create_session(&mut *tx, user_id, device_label).await?;
    tx.commit().await?;
    Ok(Json(LoginResponse { token, user_id }))
}

/// Ends the current session; its token stops working immediately.
pub async fn logout(State(state): State<AppState>, user: CurrentUser) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
        .bind(user.session_id)
        .bind(user.user_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Who the bearer token belongs to; a cheap way for the app to check its session.
pub async fn me(user: CurrentUser) -> Json<MeResponse> {
    Json(MeResponse {
        user_id: user.user_id,
    })
}
