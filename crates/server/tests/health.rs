//! Router tests against a real Postgres. Skipped when `DATABASE_URL` is unset.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use billo_api::HealthResponse;
use billo_server::app::{AppState, router};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn database_url() -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty());
    if url.is_none() {
        eprintln!("skipping: DATABASE_URL is not set");
    }
    url
}

async fn get_health(state: AppState) -> (StatusCode, HealthResponse) {
    let res = router(state)
        .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let code = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn health_ok_with_database() {
    let Some(url) = database_url() else { return };
    let db = PgPoolOptions::new().connect(&url).await.unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();

    let (code, body) = get_health(AppState { db }).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        body,
        HealthResponse {
            status: "ok".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            build: billo_server::routes::health::build_number(),
        }
    );
}

#[tokio::test]
async fn health_degraded_when_database_is_closed() {
    let Some(url) = database_url() else { return };
    let db = PgPoolOptions::new().connect(&url).await.unwrap();
    db.close().await;

    let (code, body) = get_health(AppState { db }).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body.status, "degraded");
}

#[tokio::test]
async fn users_table_exists_after_migrations() {
    let Some(url) = database_url() else { return };
    let db = PgPoolOptions::new().connect(&url).await.unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'users'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(n, 5);
}
