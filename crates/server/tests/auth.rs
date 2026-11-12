//! Session auth and dev login. Skipped when `DATABASE_URL` is unset.

mod common;

use axum::http::{Method, StatusCode};
use billo_server::auth::hash_token;
use serde_json::json;
use uuid::Uuid;

use common::{app, app_with, unique_email};

#[tokio::test]
async fn dev_login_returns_token_and_reuses_user() {
    let Some(t) = app().await else { return };
    let email = unique_email();
    let body = json!({ "email": email, "device_label": "test" });
    let a = t
        .call(
            Method::POST,
            "/api/v1/auth/dev-login",
            None,
            Some(body.clone()),
        )
        .await;
    let b = t
        .call(Method::POST, "/api/v1/auth/dev-login", None, Some(body))
        .await;
    assert_eq!(a.status, StatusCode::OK);
    assert_eq!(a.body["user_id"], b.body["user_id"]);
    assert_ne!(a.body["token"], b.body["token"]);

    // Only the hash is stored.
    let token = a.body["token"].as_str().unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions WHERE token_hash = $1")
        .bind(&hash_token(token)[..])
        .fetch_one(&t.db)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let (sub,): (String,) = sqlx::query_as("SELECT google_sub FROM users WHERE id = $1")
        .bind(a.body["user_id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&t.db)
        .await
        .unwrap();
    assert_eq!(sub, format!("dev:{email}"));
}

#[tokio::test]
async fn dev_login_rejects_bad_email() {
    let Some(t) = app().await else { return };
    for body in [json!({ "email": "nope" }), json!({ "mail": "a@b.c" })] {
        let res = t
            .call(Method::POST, "/api/v1/auth/dev-login", None, Some(body))
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(res.error_code(), "validation");
    }
}

#[tokio::test]
async fn dev_login_absent_unless_enabled() {
    let Some(t) = app_with(false).await else {
        return;
    };
    let res = t
        .call(
            Method::POST,
            "/api/v1/auth/dev-login",
            None,
            Some(json!({ "email": unique_email() })),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.error_code(), "not_found");
}

#[tokio::test]
async fn auth_required() {
    let Some(t) = app().await else { return };
    for token in [None, Some("not-a-real-token")] {
        for (method, uri) in [
            (Method::POST, "/api/v1/auth/logout"),
            (Method::GET, "/api/v1/me"),
        ] {
            let res = t.call(method, uri, token, None).await;
            assert_eq!(res.status, StatusCode::UNAUTHORIZED);
            assert_eq!(res.error_code(), "unauthorized");
        }
    }
}

#[tokio::test]
async fn logout_invalidates_token() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let res = t
        .call(Method::POST, "/api/v1/auth/logout", Some(&token), None)
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let res = t
        .call(Method::POST, "/api/v1/auth/logout", Some(&token), None)
        .await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_session_is_rejected_and_removed() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let hash = hash_token(&token);
    sqlx::query(
        "UPDATE sessions SET expires_at = now() - interval '1 second' WHERE token_hash = $1",
    )
    .bind(&hash[..])
    .execute(&t.db)
    .await
    .unwrap();
    let res = t
        .call(Method::POST, "/api/v1/auth/logout", Some(&token), None)
        .await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions WHERE token_hash = $1")
        .bind(&hash[..])
        .fetch_one(&t.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn session_slides_at_most_hourly() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let hash = hash_token(&token);
    let expiry = || async {
        let (e,): (time::OffsetDateTime,) =
            sqlx::query_as("SELECT expires_at FROM sessions WHERE token_hash = $1")
                .bind(&hash[..])
                .fetch_one(&t.db)
                .await
                .unwrap();
        e
    };
    // A session used 2 hours ago that expires in 10 days gets pushed out to 90 days.
    sqlx::query(
        "UPDATE sessions SET last_used_at = now() - interval '2 hours', \
         expires_at = now() + interval '10 days' WHERE token_hash = $1",
    )
    .bind(&hash[..])
    .execute(&t.db)
    .await
    .unwrap();
    let before = expiry().await;
    let res = t.get("/api/v1/me", &token).await;
    assert_ne!(res.status, StatusCode::UNAUTHORIZED);
    let after = expiry().await;
    assert!(after - before > time::Duration::days(79), "not extended");

    // Used again right away: no write.
    sqlx::query(
        "UPDATE sessions SET expires_at = now() + interval '10 days' WHERE token_hash = $1",
    )
    .bind(&hash[..])
    .execute(&t.db)
    .await
    .unwrap();
    let before = expiry().await;
    t.get("/api/v1/me", &token).await;
    assert_eq!(expiry().await, before);
    assert_eq!(
        t.get("/api/v1/me", &token).await.body["user_id"],
        user_id.to_string()
    );
}
