//! Talking to the Billo server: typed calls for every `/api/v1` resource.
//!
//! Reads go straight to the server for now; the offline cache and sync outbox
//! (architecture §5) are the next step and will sit behind this module.

use std::fmt;

use billo_api::{
    AccountData, BudgetData, BudgetQuery, CategoryData, DevLoginRequest, ErrorBody, ErrorCode,
    HealthResponse, InstallmentPlanData, List, LoginResponse, MeResponse, Put, Record,
    TransactionData, TransactionQuery,
};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

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

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(m) => write!(f, "Can't reach the server ({m})"),
            Self::Unauthorized => f.write_str("Your session has ended. Please sign in again."),
            Self::NotFound => f.write_str("Not found"),
            Self::Validation(m) => write!(f, "Not saved: {m}"),
            Self::Conflict(m) => write!(f, "Conflict: {m}"),
            Self::Server(status, m) => write!(f, "Server error {status}: {m}"),
            Self::Decode(m) => write!(f, "Unexpected response from the server ({m})"),
        }
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

/// A new client-generated id (UUIDv7, time-ordered).
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// Query string for `GET /transactions`. Only uuids and ISO dates, so no escaping.
fn transaction_query(q: &TransactionQuery) -> String {
    let mut parts = Vec::new();
    if let Some(a) = q.account_id {
        parts.push(format!("account_id={a}"));
    }
    if let Some(d) = q.from {
        parts.push(format!("from={d}"));
    }
    if let Some(d) = q.to {
        parts.push(format!("to={d}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
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

    pub async fn me(&self) -> Result<MeResponse, ApiError> {
        self.json(self.http.get(self.url("/me"))).await
    }

    async fn list<D: DeserializeOwned>(&self, path: &str) -> Result<Vec<Record<D>>, ApiError> {
        let list: List<D> = self.json(self.http.get(self.url(path))).await?;
        Ok(list.items)
    }

    async fn get<D: DeserializeOwned>(&self, path: &str, id: Uuid) -> Result<Record<D>, ApiError> {
        self.json(self.http.get(self.url(&format!("{path}/{id}"))))
            .await
    }

    async fn put<D: Serialize + DeserializeOwned>(
        &self,
        path: &str,
        id: Uuid,
        base_version: Option<i64>,
        data: D,
    ) -> Result<Record<D>, ApiError> {
        let body = Put { base_version, data };
        self.json(self.http.put(self.url(&format!("{path}/{id}"))).json(&body))
            .await
    }

    async fn delete(&self, path: &str, id: Uuid) -> Result<(), ApiError> {
        self.empty(self.http.delete(self.url(&format!("{path}/{id}"))))
            .await
    }

    pub async fn accounts(&self) -> Result<Vec<Record<AccountData>>, ApiError> {
        self.list("/accounts").await
    }

    pub async fn account(&self, id: Uuid) -> Result<Record<AccountData>, ApiError> {
        self.get("/accounts", id).await
    }

    /// Creates (`base_version: None`) or updates the account with this id.
    pub async fn put_account(
        &self,
        id: Uuid,
        base_version: Option<i64>,
        data: AccountData,
    ) -> Result<Record<AccountData>, ApiError> {
        self.put("/accounts", id, base_version, data).await
    }

    pub async fn delete_account(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete("/accounts", id).await
    }

    pub async fn categories(&self) -> Result<Vec<Record<CategoryData>>, ApiError> {
        self.list("/categories").await
    }

    pub async fn put_category(
        &self,
        id: Uuid,
        base_version: Option<i64>,
        data: CategoryData,
    ) -> Result<Record<CategoryData>, ApiError> {
        self.put("/categories", id, base_version, data).await
    }

    pub async fn delete_category(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete("/categories", id).await
    }

    /// Newest first (by `occurred_on`).
    pub async fn transactions(
        &self,
        q: &TransactionQuery,
    ) -> Result<Vec<Record<TransactionData>>, ApiError> {
        self.list(&format!("/transactions{}", transaction_query(q)))
            .await
    }

    pub async fn put_transaction(
        &self,
        id: Uuid,
        base_version: Option<i64>,
        data: TransactionData,
    ) -> Result<Record<TransactionData>, ApiError> {
        self.put("/transactions", id, base_version, data).await
    }

    pub async fn delete_transaction(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete("/transactions", id).await
    }

    pub async fn installment_plans(&self) -> Result<Vec<Record<InstallmentPlanData>>, ApiError> {
        self.list("/installment-plans").await
    }

    pub async fn put_installment_plan(
        &self,
        id: Uuid,
        base_version: Option<i64>,
        data: InstallmentPlanData,
    ) -> Result<Record<InstallmentPlanData>, ApiError> {
        self.put("/installment-plans", id, base_version, data).await
    }

    pub async fn delete_installment_plan(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete("/installment-plans", id).await
    }

    pub async fn budgets(&self, q: &BudgetQuery) -> Result<Vec<Record<BudgetData>>, ApiError> {
        let path = match q.period_month {
            Some(m) => format!("/budgets?period_month={m}"),
            None => "/budgets".to_owned(),
        };
        self.list(&path).await
    }

    pub async fn put_budget(
        &self,
        id: Uuid,
        base_version: Option<i64>,
        data: BudgetData,
    ) -> Result<Record<BudgetData>, ApiError> {
        self.put("/budgets", id, base_version, data).await
    }

    pub async fn delete_budget(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete("/budgets", id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

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
    fn maps_non_json_errors_by_status() {
        assert_eq!(error_from(401, ""), ApiError::Unauthorized);
        assert_eq!(error_from(404, "Not Found"), ApiError::NotFound);
        assert_eq!(
            error_from(502, "Bad Gateway\n"),
            ApiError::Server(502, "Bad Gateway".into())
        );
    }

    #[test]
    fn builds_transaction_query() {
        assert_eq!(transaction_query(&TransactionQuery::default()), "");
        let q = TransactionQuery {
            account_id: Some(Uuid::nil()),
            from: Some(date!(2026 - 11 - 01)),
            to: None,
        };
        assert_eq!(
            transaction_query(&q),
            "?account_id=00000000-0000-0000-0000-000000000000&from=2026-11-01"
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
