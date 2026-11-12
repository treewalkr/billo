//! Generic CRUD handlers for every [`Resource`]:
//!
//! - `GET    /api/v1/<resource>`       live records (filters per resource)
//! - `GET    /api/v1/<resource>/{id}`  one live record
//! - `PUT    /api/v1/<resource>/{id}`  create (201) or update (200); sets
//!   `x-billo-conflict: 1` when `base_version` was stale
//! - `DELETE /api/v1/<resource>/{id}`  soft delete (204)

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use billo_api::{CONFLICT_HEADER, List, Put, Record};
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::extract::{Json, Path, Query};
use crate::repo::{self, Resource};

pub fn routes<R: Resource>(path: &str) -> Router<AppState> {
    Router::new().route(path, get(list::<R>)).route(
        &format!("{path}/{{id}}"),
        get(get_one::<R>).put(put::<R>).delete(delete::<R>),
    )
}

async fn list<R: Resource>(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(filter): Query<R::Filter>,
) -> ApiResult<Json<List<R::Data>>> {
    let items = repo::list::<R>(&state.db, user.user_id, &filter).await?;
    Ok(Json(List { items }))
}

async fn get_one<R: Resource>(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Record<R::Data>>> {
    Ok(Json(repo::get::<R>(&state.db, user.user_id, id).await?))
}

async fn put<R: Resource>(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<Put<R::Data>>,
) -> ApiResult<Response> {
    let out = repo::put::<R>(&state.db, user.user_id, id, body).await?;
    let status = if out.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut res = (status, Json(out.record)).into_response();
    if out.conflict {
        res.headers_mut()
            .insert(CONFLICT_HEADER, HeaderValue::from_static("1"));
    }
    Ok(res)
}

async fn delete<R: Resource>(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    repo::delete::<R>(&state.db, user.user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
