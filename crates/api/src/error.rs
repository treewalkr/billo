//! Error body returned by every failing `/api/v1` request.

use serde::{Deserialize, Serialize};

/// `{ "error": { "code": "...", "message": "..." } }`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorDetail {
    pub code: ErrorCode,
    /// Human-readable, for logs and debugging; not meant to be shown to users as-is.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    Validation,
    Unauthorized,
    Conflict,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_json_shape() {
        let e = ErrorBody {
            error: ErrorDetail {
                code: ErrorCode::NotFound,
                message: "not found".into(),
            },
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"error":{"code":"not_found","message":"not found"}}"#
        );
    }
}
