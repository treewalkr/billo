//! Shared helpers for router tests against a real Postgres.
//! Tests skip (return early) when `DATABASE_URL` is unset.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use billo_server::app::{AppState, router};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

pub struct TestApp {
    pub db: PgPool,
    pub app: Router,
}

pub struct Res {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl Res {
    pub fn conflict_flag(&self) -> bool {
        self.headers
            .get("x-billo-conflict")
            .is_some_and(|v| v == "1")
    }

    pub fn error_code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }
}

/// App with dev login enabled, or `None` (test skipped) without `DATABASE_URL`.
pub async fn app() -> Option<TestApp> {
    app_with(true).await
}

pub async fn app_with(dev_login: bool) -> Option<TestApp> {
    let Some(url) = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty()) else {
        eprintln!("skipping: DATABASE_URL is not set");
        return None;
    };
    let db = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();
    let app = router(AppState {
        db: db.clone(),
        dev_login,
        google: None,
        api_docs: false,
    });
    Some(TestApp { db, app })
}

/// A unique email so parallel tests never share a user.
pub fn unique_email() -> String {
    format!("test-{}@example.com", Uuid::now_v7())
}

impl TestApp {
    pub async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Res {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&v).unwrap())
            }
            None => Body::empty(),
        };
        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Res {
            status,
            headers,
            body,
        }
    }

    /// Signs in a fresh user; returns `(token, user_id)`.
    pub async fn login(&self) -> (String, Uuid) {
        let res = self
            .call(
                Method::POST,
                "/api/v1/auth/dev-login",
                None,
                Some(serde_json::json!({ "email": unique_email() })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.body);
        (
            res.body["token"].as_str().unwrap().to_owned(),
            res.body["user_id"].as_str().unwrap().parse().unwrap(),
        )
    }

    pub async fn get(&self, uri: &str, token: &str) -> Res {
        self.call(Method::GET, uri, Some(token), None).await
    }

    pub async fn put(&self, uri: &str, token: &str, body: Value) -> Res {
        self.call(Method::PUT, uri, Some(token), Some(body)).await
    }

    pub async fn delete(&self, uri: &str, token: &str) -> Res {
        self.call(Method::DELETE, uri, Some(token), None).await
    }

    /// `(entity, version, op)` rows of the user's change feed, oldest first.
    pub async fn change_log(&self, user_id: Uuid, entity_id: Uuid) -> Vec<(String, i64, String)> {
        sqlx::query_as(
            "SELECT entity, version, op FROM change_log \
             WHERE user_id = $1 AND entity_id = $2 ORDER BY seq",
        )
        .bind(user_id)
        .bind(entity_id)
        .fetch_all(&self.db)
        .await
        .unwrap()
    }
}
