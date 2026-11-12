//! Wrappers around Axum's extractors that reject with [`ApiError`] (JSON body)
//! instead of Axum's plain-text rejections.

use aide::generate::GenContext;
use aide::openapi::{Operation, Response as ApiResponse, StatusCode as ApiStatus};
use aide::{OperationInput, OperationOutput};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{ApiError, early_error_response};

pub struct Json<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Json<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        let axum::Json(v) = axum::Json::<T>::from_request(req, state).await?;
        Ok(Self(v))
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

pub struct Query<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Query<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let axum::extract::Query(v) =
            axum::extract::Query::<T>::from_request_parts(parts, state).await?;
        Ok(Self(v))
    }
}

pub struct Path<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for Path<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let axum::extract::Path(v) =
            axum::extract::Path::<T>::from_request_parts(parts, state).await?;
        Ok(Self(v))
    }
}

// OpenAPI docs: the same request and response shapes as Axum's extractors, but
// rejections are documented as our JSON error body.

impl<T: JsonSchema> OperationInput for Json<T> {
    fn operation_input(ctx: &mut GenContext, operation: &mut Operation) {
        axum::Json::<T>::operation_input(ctx, operation);
    }

    fn inferred_early_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<ApiStatus>, ApiResponse)> {
        early_error_response(
            ctx,
            operation,
            422,
            "The body is not valid JSON of the expected shape",
        )
    }
}

impl<T: JsonSchema> OperationOutput for Json<T> {
    type Inner = T;

    fn operation_response(ctx: &mut GenContext, operation: &mut Operation) -> Option<ApiResponse> {
        axum::Json::<T>::operation_response(ctx, operation)
    }

    fn inferred_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<ApiStatus>, ApiResponse)> {
        Self::operation_response(ctx, operation)
            .map(|res| (Some(ApiStatus::Code(200)), res))
            .into_iter()
            .collect()
    }
}

impl<T: JsonSchema> OperationInput for Query<T> {
    fn operation_input(ctx: &mut GenContext, operation: &mut Operation) {
        axum::extract::Query::<T>::operation_input(ctx, operation);
    }

    fn inferred_early_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<ApiStatus>, ApiResponse)> {
        early_error_response(ctx, operation, 422, "A query parameter is not valid")
    }
}

impl<T: JsonSchema> OperationInput for Path<T> {
    fn operation_input(ctx: &mut GenContext, operation: &mut Operation) {
        axum::extract::Path::<T>::operation_input(ctx, operation);
    }

    fn inferred_early_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<ApiStatus>, ApiResponse)> {
        early_error_response(ctx, operation, 404, "No such record")
    }
}
