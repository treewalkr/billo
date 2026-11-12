//! Billo API: request/response types shared by the server and the app.
//!
//! Everything here is part of the `/api/v1` contract and must stay backward-compatible.

use serde::{Deserialize, Serialize};

pub mod auth;
pub mod error;
pub mod records;
pub mod resources;
pub mod rules;
pub mod sync;

pub use auth::{AuthMethods, DevLoginRequest, GoogleLoginRequest, LoginResponse, MeResponse};
pub use error::{ErrorBody, ErrorCode, ErrorDetail};
pub use records::{CONFLICT_HEADER, List, Money, Put, Record};
pub use resources::{
    AccountData, AccountKind, BudgetData, BudgetQuery, CategoryData, CategoryKind,
    InstallmentPlanData, Remainder, TransactionData, TransactionKind, TransactionQuery,
};
pub use rules::AccountRules;

/// Body of `GET /api/v1/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// `"ok"`, or `"degraded"` when a dependency (e.g. the database) is down.
    pub status: String,
    /// Workspace version, `X.Y.Z`.
    pub version: String,
    /// Build number; only ever goes up.
    pub build: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use uuid::Uuid;

    #[test]
    fn record_json_shape() {
        let r = Record {
            id: Uuid::nil(),
            version: 2,
            created_at: datetime!(2026-11-12 10:00 UTC),
            updated_at: datetime!(2026-11-12 10:05 UTC),
            data: AccountData {
                kind: AccountKind::Cash,
                name: "Wallet".into(),
                currency: "THB".into(),
                rules: AccountRules::default(),
                opened_on: Some(date!(2026 - 01 - 05)),
                closed_on: None,
            },
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "00000000-0000-0000-0000-000000000000",
                "version": 2,
                "created_at": "2026-11-12T10:00:00Z",
                "updated_at": "2026-11-12T10:05:00Z",
                "kind": "cash",
                "name": "Wallet",
                "currency": "THB",
                "rules": { "v": 1 },
                "opened_on": "2026-01-05",
                "closed_on": null,
            })
        );
        assert_eq!(
            serde_json::from_value::<Record<AccountData>>(json).unwrap(),
            r
        );
    }

    #[test]
    fn put_reads_base_version_and_defaults() {
        let p: Put<AccountData> = serde_json::from_str(
            r#"{"base_version": 3, "kind": "bank", "name": "x", "currency": "THB"}"#,
        )
        .unwrap();
        assert_eq!(p.base_version, Some(3));
        assert_eq!(p.data.rules, AccountRules::default());
    }

    #[test]
    fn health_json_shape() {
        let h = HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
            build: 1,
        };
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, r#"{"status":"ok","version":"0.1.0","build":1}"#);
        assert_eq!(serde_json::from_str::<HealthResponse>(&json).unwrap(), h);
    }
}
