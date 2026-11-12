//! Router construction, separate from `main` so tests can drive it directly.

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use axum::routing::{get, post};
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::google::Google;
use crate::routes;

use crate::error;
use crate::resources::accounts::Accounts;
use crate::resources::budgets::Budgets;
use crate::resources::categories::Categories;
use crate::resources::installment_plans::InstallmentPlans;
use crate::resources::transactions::Transactions;
use crate::routes::resources::routes as resource_routes;

#[derive(Debug, Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Registers `POST /api/v1/auth/dev-login`. Never enable in production.
    pub dev_login: bool,
    /// Registers the Google sign-in routes when set.
    pub google: Option<Arc<Google>>,
}

impl AppState {
    /// State with dev login and Google sign-in disabled.
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            dev_login: false,
            google: None,
        }
    }
}

pub fn router(state: AppState) -> Router {
    let mut api = Router::new()
        .route("/health", get(routes::health::health))
        .route("/auth/methods", get(routes::google::methods))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/me", get(routes::auth::me))
        .merge(resource_routes::<Accounts>("/accounts"))
        .merge(resource_routes::<Categories>("/categories"))
        .merge(resource_routes::<Transactions>("/transactions"))
        .merge(resource_routes::<InstallmentPlans>("/installment-plans"))
        .merge(resource_routes::<Budgets>("/budgets"))
        .merge(routes::sync::routes());
    if state.dev_login {
        api = api.route("/auth/dev-login", post(routes::auth::dev_login));
    }
    if state.google.is_some() {
        api = api
            .route("/auth/google/start", get(routes::google::start))
            .route("/auth/google/callback", get(routes::google::callback))
            .route("/auth/google", post(routes::google::native));
    }
    Router::new()
        .nest("/api/v1", api)
        .fallback(error::not_found)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Lets the listed browser origins call the API. Meant for development (the web app
/// served by `dx serve` on another port); with no origins the router is unchanged.
pub fn with_cors(router: Router, origins: &[HeaderValue]) -> Router {
    if origins.is_empty() {
        return router;
    }
    router.layer(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins.iter().cloned()))
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn plain() -> Router {
        Router::new().route("/ping", get(|| async { "pong" }))
    }

    async fn allow_origin(app: Router, origin: &str) -> Option<HeaderValue> {
        let res = app
            .oneshot(
                Request::get("/ping")
                    .header(header::ORIGIN, origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        res.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .cloned()
    }

    #[tokio::test]
    async fn no_origins_means_no_cors_headers() {
        assert_eq!(
            allow_origin(with_cors(plain(), &[]), "http://localhost:8081").await,
            None
        );
    }

    #[tokio::test]
    async fn allows_listed_origin_only() {
        let origins = [HeaderValue::from_static("http://localhost:8081")];
        assert_eq!(
            allow_origin(with_cors(plain(), &origins), "http://localhost:8081").await,
            Some(HeaderValue::from_static("http://localhost:8081"))
        );
        assert_eq!(
            allow_origin(with_cors(plain(), &origins), "http://evil.example").await,
            None
        );
    }
}
