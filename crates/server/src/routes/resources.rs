//! Generic CRUD handlers for every [`Resource`]:
//!
//! - `GET    /api/v1/<resource>`       live records (filters per resource)
//! - `GET    /api/v1/<resource>/{id}`  one live record
//! - `PUT    /api/v1/<resource>/{id}`  create (201) or update (200); sets
//!   `x-billo-conflict: 1` when `base_version` was stale
//! - `DELETE /api/v1/<resource>/{id}`  soft delete (204)

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use aide::openapi::{Header, HeaderStyle, ParameterSchemaOrContent, ReferenceOr, SchemaObject};
use aide::transform::TransformOperation;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, NoContent, Response};
use billo_api::{CONFLICT_HEADER, List, Put, Record};
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::extract::{Json, Path, Query};
use crate::repo::{self, Resource};

/// How a resource is named in the docs, e.g. `("Accounts", "account", "accounts")`.
#[derive(Debug, Clone, Copy)]
pub struct Names {
    /// Tag that groups the resource's operations.
    pub tag: &'static str,
    pub one: &'static str,
    pub many: &'static str,
}

pub fn routes<R: Resource>(path: &str, names: Names) -> ApiRouter<AppState> {
    let Names { tag, one, many } = names;
    ApiRouter::new()
        .api_route(
            path,
            get_with(list::<R>, move |op| {
                op.tag(tag)
                    .summary(&format!("List {many}"))
                    .description("Live (not deleted) records, oldest first.")
            }),
        )
        .api_route(
            &format!("{path}/{{id}}"),
            get_with(get_one::<R>, move |op| {
                op.tag(tag).summary(&format!("Get {}", article(one)))
            })
            .put_with(put::<R>, move |op| put_docs::<R>(op, tag, one))
            .delete_with(delete::<R>, move |op| {
                op.tag(tag)
                    .summary(&format!("Delete {}", article(one)))
                    .description("Soft delete; deleting an already deleted record is a no-op.")
                    .response_with::<204, NoContent, _>(|r| r.description("Deleted"))
            }),
        )
}

/// "an account", "a budget".
fn article(noun: &str) -> String {
    if noun.starts_with(['a', 'e', 'i', 'o', 'u']) {
        format!("an {noun}")
    } else {
        format!("a {noun}")
    }
}

fn put_docs<'t, R: Resource>(
    op: TransformOperation<'t>,
    tag: &str,
    one: &str,
) -> TransformOperation<'t> {
    let conflict_header = || {
        ReferenceOr::Item(Header {
            description: Some(
                "`1` when `base_version` was stale; the write was still applied (last write wins)."
                    .into(),
            ),
            style: HeaderStyle::Simple,
            required: false,
            deprecated: None,
            format: ParameterSchemaOrContent::Schema(SchemaObject {
                json_schema: schemars::json_schema!({ "type": "string", "enum": ["1"] }),
                example: None,
                external_docs: None,
            }),
            example: None,
            examples: Default::default(),
            extensions: Default::default(),
        })
    };
    op.tag(tag)
        .summary(&format!("Create or update {}", article(one)))
        .description(
            "Ids are UUIDv7s made by the client, so a retried PUT never creates a duplicate. \
             Sending the same data again returns the record unchanged.",
        )
        .response_with::<201, Json<Record<R::Data>>, _>(|mut r| {
            r.inner()
                .headers
                .insert(CONFLICT_HEADER.into(), conflict_header());
            r.description("Created")
        })
        .response_with::<200, Json<Record<R::Data>>, _>(|mut r| {
            r.inner()
                .headers
                .insert(CONFLICT_HEADER.into(), conflict_header());
            r.description("Updated (or unchanged)")
        })
}

/// The `{id}` path segment.
#[derive(Debug, Deserialize, JsonSchema)]
struct RecordId {
    /// The record's UUIDv7, made by the client.
    id: Uuid,
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
    Path(RecordId { id }): Path<RecordId>,
) -> ApiResult<Json<Record<R::Data>>> {
    Ok(Json(repo::get::<R>(&state.db, user.user_id, id).await?))
}

async fn put<R: Resource>(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(RecordId { id }): Path<RecordId>,
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
    Path(RecordId { id }): Path<RecordId>,
) -> ApiResult<StatusCode> {
    repo::delete::<R>(&state.db, user.user_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
