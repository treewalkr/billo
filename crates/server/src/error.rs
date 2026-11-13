//! The one error type handlers return, rendered as `{ "error": { "code", "message" } }`.
//!
//! Messages never contain amounts, notes or account names (architecture §7), and
//! database errors are logged by code and constraint only.

use aide::OperationOutput;
use aide::generate::GenContext;
use aide::openapi::{Operation, Response as ApiResponse, StatusCode as ApiStatus};
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use billo_api::{ErrorBody, ErrorCode, ErrorDetail};

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    Validation(String),
    Unauthorized,
    /// A sign-in proof (e.g. a Google ID token) was refused; the reason is shown.
    SignInFailed(&'static str),
    Conflict(String),
    Internal,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// The error as it appears in a response body.
    pub fn detail(&self) -> ErrorDetail {
        let (_, code, message) = self.parts();
        ErrorDetail {
            code,
            message: message.to_owned(),
        }
    }

    fn parts(&self) -> (StatusCode, ErrorCode, &str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound, "not found"),
            Self::Validation(m) => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::Validation, m),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                ErrorCode::Unauthorized,
                "missing, invalid or expired session",
            ),
            Self::SignInFailed(m) => (StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized, m),
            Self::Conflict(m) => (StatusCode::CONFLICT, ErrorCode::Conflict, m),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                "internal error",
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        let body = ErrorBody {
            error: ErrorDetail {
                code,
                message: message.to_owned(),
            },
        };
        (status, Json(body)).into_response()
    }
}

/// An error response for the OpenAPI docs: the [`ErrorBody`] schema with `description`.
pub fn error_response(ctx: &mut GenContext, description: &str) -> ApiResponse {
    let mut res = Json::<ErrorBody>::operation_response(ctx, &mut Operation::default())
        .expect("Json documents a response");
    res.description = description.to_owned();
    res
}

/// An extractor's rejection for the OpenAPI docs, unless the handler's own
/// [`ApiError`] responses already list that status.
pub fn early_error_response(
    ctx: &mut GenContext,
    operation: &Operation,
    code: u16,
    description: &str,
) -> Vec<(Option<ApiStatus>, ApiResponse)> {
    let status = ApiStatus::Code(code);
    let listed = operation
        .responses
        .as_ref()
        .is_some_and(|r| r.responses.contains_key(&status));
    if listed {
        Vec::new()
    } else {
        vec![(Some(status), error_response(ctx, description))]
    }
}

impl OperationOutput for ApiError {
    type Inner = ErrorBody;

    fn operation_response(ctx: &mut GenContext, operation: &mut Operation) -> Option<ApiResponse> {
        Json::<ErrorBody>::operation_response(ctx, operation)
    }

    fn inferred_responses(
        ctx: &mut GenContext,
        _operation: &mut Operation,
    ) -> Vec<(Option<ApiStatus>, ApiResponse)> {
        [
            (401, "Missing, invalid or expired session"),
            (404, "Not found"),
            (
                409,
                "The id belongs to another user, or the record is a duplicate",
            ),
            (422, "The request is not valid"),
            (500, "Internal error; worth retrying"),
        ]
        .into_iter()
        .map(|(code, description)| {
            (
                Some(ApiStatus::Code(code)),
                error_response(ctx, description),
            )
        })
        .collect()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let Some(db) = e.as_database_error() {
            let constraint = db.constraint().unwrap_or("");
            match db.code().as_deref() {
                // foreign_key_violation: a referenced row doesn't exist for this user.
                Some("23503") => {
                    return Self::validation(format!(
                        "referenced record does not exist ({constraint})"
                    ));
                }
                Some("23505") => {
                    return Self::Conflict(format!("duplicate record ({constraint})"));
                }
                Some("23514") => {
                    return Self::validation(format!("invalid value ({constraint})"));
                }
                code => {
                    tracing::error!(code = ?code, constraint, "database error");
                    return Self::Internal;
                }
            }
        }
        // Not a database error (pool, I/O, decoding): these don't carry user data.
        tracing::error!(error = %e, "database error");
        Self::Internal
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        Self::Validation(r.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        Self::Validation(r.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        // A malformed id can't name an existing record.
        Self::NotFound
    }
}

/// JSON fallback for unknown routes.
pub async fn not_found() -> ApiError {
    ApiError::NotFound
}
