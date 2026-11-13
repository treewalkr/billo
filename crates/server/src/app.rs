//! Router construction, separate from `main` so tests can drive it directly.

use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use aide::openapi::{OpenApi, SecurityScheme};
use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderValue, Method, header};
use axum::response::{Html, NoContent};
use axum::routing::get;
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::google::Google;
use crate::routes;

use crate::auth::SECURITY_SCHEME;
use crate::error;
use crate::resources::accounts::Accounts;
use crate::resources::budgets::Budgets;
use crate::resources::categories::Categories;
use crate::resources::installment_plans::InstallmentPlans;
use crate::resources::transactions::Transactions;
use crate::routes::resources::{Names, routes as resource_routes};

#[derive(Debug, Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Registers `POST /api/v1/auth/dev-login`. Never enable in production.
    pub dev_login: bool,
    /// Registers the Google sign-in routes when set.
    pub google: Option<Arc<Google>>,
    /// Serves the OpenAPI spec at [`OPENAPI_PATH`] and Scalar docs at [`DOCS_PATH`].
    pub api_docs: bool,
}

impl AppState {
    /// State with dev login, Google sign-in and the API docs disabled.
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            dev_login: false,
            google: None,
            api_docs: false,
        }
    }
}

/// The OpenAPI spec, when `api_docs` is on. Outside `/api/v1`: not part of the contract.
pub const OPENAPI_PATH: &str = "/api/openapi.json";
/// The Scalar API reference page, when `api_docs` is on.
pub const DOCS_PATH: &str = "/api/docs";

pub fn router(state: AppState) -> Router {
    let (router, _docs) = router_and_docs(state);
    router
}

/// The router and its OpenAPI description, built from the same route table so the
/// docs can't drift from the code.
pub fn router_and_docs(state: AppState) -> (Router, OpenApi) {
    // Name shared types under `components.schemas` instead of inlining them.
    aide::generate::extract_schemas(true);

    let mut api = ApiRouter::new()
        .api_route(
            "/health",
            get_with(routes::health::health, routes::health::docs),
        )
        .api_route(
            "/auth/methods",
            get_with(routes::google::methods, |op| {
                op.tag("Auth").summary("List sign-in methods")
            }),
        )
        .api_route(
            "/auth/logout",
            post_with(routes::auth::logout, |op| {
                op.tag("Auth")
                    .summary("Sign out")
                    .description("Ends the current session; its token stops working immediately.")
                    .response_with::<204, NoContent, _>(|r| r.description("Signed out"))
            }),
        )
        .api_route(
            "/me",
            get_with(routes::auth::me, |op| {
                op.tag("Auth")
                    .summary("Who am I")
                    .description("A cheap way for the app to check its session.")
            }),
        )
        .merge(resource_routes::<Accounts>(
            "/accounts",
            Names {
                tag: "Accounts",
                one: "account",
                many: "accounts",
            },
        ))
        .merge(resource_routes::<Categories>(
            "/categories",
            Names {
                tag: "Categories",
                one: "category",
                many: "categories",
            },
        ))
        .merge(resource_routes::<Transactions>(
            "/transactions",
            Names {
                tag: "Transactions",
                one: "transaction",
                many: "transactions",
            },
        ))
        .merge(resource_routes::<InstallmentPlans>(
            "/installment-plans",
            Names {
                tag: "Installment plans",
                one: "installment plan",
                many: "installment plans",
            },
        ))
        .merge(resource_routes::<Budgets>(
            "/budgets",
            Names {
                tag: "Budgets",
                one: "budget",
                many: "budgets",
            },
        ))
        .merge(routes::sync::routes());
    if state.dev_login {
        api = api.api_route(
            "/auth/dev-login",
            post_with(routes::auth::dev_login, |op| {
                op.tag("Auth")
                    .summary("Sign in without a password (development only)")
                    .description("Only registered when the server runs with `DEV_LOGIN=true`.")
            }),
        );
    }
    if state.google.is_some() {
        api = api
            .api_route(
                "/auth/google/start",
                get_with(routes::google::start, routes::google::start_docs),
            )
            .api_route(
                "/auth/google/callback",
                get_with(routes::google::callback, routes::google::callback_docs),
            )
            .api_route(
                "/auth/google",
                post_with(routes::google::native, |op| {
                    op.tag("Auth")
                        .summary("Sign in with a Google ID token")
                        .description(
                            "For the platform's native Google sign-in. Accepts tokens issued to \
                             the web or iOS client; checks `nonce` only when given.",
                        )
                }),
            );
    }

    let mut docs = OpenApi::default();
    let mut router = ApiRouter::new()
        .nest("/api/v1", api)
        .finish_api_with(&mut docs, |t| {
            t.title("Billo API")
                .version(routes::health::VERSION)
                .description(
                    "Sign in to get a session token, then send it as \
                     `Authorization: Bearer <token>`. Errors are \
                     `{\"error\": {\"code\", \"message\"}}`.",
                )
                .security_scheme(
                    SECURITY_SCHEME,
                    SecurityScheme::Http {
                        scheme: "bearer".into(),
                        bearer_format: None,
                        description: Some("A session token from a sign-in route.".into()),
                        extensions: Default::default(),
                    },
                )
        });
    if state.api_docs {
        router = router.merge(docs_routes(&docs));
    }
    let router = router
        .fallback(error::not_found)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    (router, docs)
}

/// Scalar API reference page. Our own template (aide's hardcodes its theme), with
/// Scalar loaded from jsDelivr at a pinned version.
const DOCS_PAGE: &str = r#"<!doctype html>
<html>
  <head>
    <title>Billo API</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
  </head>
  <body>
    <div id="app"></div>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.78.1"></script>
    <script>
      Scalar.createApiReference('#app', {
        url: '{spec_url}',
        theme: 'elysiajs',
      })
    </script>
  </body>
</html>
"#;

/// The spec (serialised once) and the Scalar page that renders it.
fn docs_routes(docs: &OpenApi) -> Router<AppState> {
    let spec = Bytes::from(serde_json::to_vec(docs).expect("the OpenAPI spec serialises"));
    let page = Bytes::from(DOCS_PAGE.replace("{spec_url}", OPENAPI_PATH));
    Router::new()
        .route(
            OPENAPI_PATH,
            get(move || async move { ([(header::CONTENT_TYPE, "application/json")], spec) }),
        )
        .route(DOCS_PATH, get(move || async move { Html(page) }))
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
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// The router's docs, optionally with every optional route on. No database or
    /// network is touched.
    fn spec(all_routes: bool) -> serde_json::Value {
        aide::generate::on_error(|e| panic!("OpenAPI generation: {e}"));
        let db = PgPool::connect_lazy("postgres://localhost/unused").unwrap();
        let google = all_routes.then(|| {
            let config = crate::config::GoogleConfig {
                client_id: "web".into(),
                client_secret: "secret".into(),
                ios_client_id: None,
                redirect_uri: "http://localhost:8080/api/v1/auth/google/callback".into(),
                app_origins: vec!["http://localhost:8081".into()],
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                jwks_url: "https://www.googleapis.com/oauth2/v3/certs".into(),
                issuers: Vec::new(),
            };
            Arc::new(Google::new(config).unwrap())
        });
        let (_, docs) = router_and_docs(AppState {
            dev_login: all_routes,
            google,
            ..AppState::new(db)
        });
        serde_json::to_value(docs).unwrap()
    }

    #[tokio::test]
    async fn docs_describe_every_route() {
        let spec = spec(true);
        let paths = &spec["paths"];
        for path in [
            "/api/v1/health",
            "/api/v1/me",
            "/api/v1/auth/dev-login",
            "/api/v1/auth/google",
            "/api/v1/auth/google/start",
            "/api/v1/auth/google/callback",
            "/api/v1/accounts",
            "/api/v1/accounts/{id}",
            "/api/v1/installment-plans/{id}",
            "/api/v1/sync/push",
            "/api/v1/sync/pull",
        ] {
            assert!(paths[path].is_object(), "{path} is missing");
        }
        // Resources are named per type, with their fields flattened beside the metadata.
        let record = &spec["components"]["schemas"]["AccountDataRecord"]["properties"];
        assert_eq!(record["opened_on"]["format"], "date");
        assert_eq!(record["created_at"]["format"], "date-time");
        assert_eq!(
            spec["components"]["securitySchemes"]["bearer"]["scheme"],
            "bearer"
        );

        let put = &paths["/api/v1/accounts/{id}"]["put"];
        assert_eq!(put["tags"][0], "Accounts");
        assert_eq!(put["parameters"][0]["name"], "id");
        assert!(put["responses"]["201"]["headers"]["x-billo-conflict"].is_object());
        assert!(put["responses"]["422"].is_object());

        let callback = &paths["/api/v1/auth/google/callback"]["get"]["responses"];
        assert!(callback["302"].is_object());
        assert!(callback["400"]["content"]["text/html"].is_object());
        let health = &paths["/api/v1/health"]["get"]["responses"];
        assert!(health["200"].is_object() && health["503"].is_object());
    }

    #[tokio::test]
    async fn docs_mark_authed_routes_only() {
        let spec = spec(false);
        let paths = &spec["paths"];
        let bearer = serde_json::json!([{ "bearer": [] }]);
        assert_eq!(paths["/api/v1/me"]["get"]["security"], bearer);
        assert_eq!(paths["/api/v1/sync/push"]["post"]["security"], bearer);
        assert!(paths["/api/v1/health"]["get"]["security"].is_null());
        assert!(paths["/api/v1/auth/methods"]["get"]["security"].is_null());
        assert!(paths["/api/v1/auth/dev-login"].is_null());
    }

    async fn get_docs(api_docs: bool, path: &str) -> (StatusCode, String, Bytes) {
        let db = PgPool::connect_lazy("postgres://localhost/unused").unwrap();
        let res = router(AppState {
            api_docs,
            ..AppState::new(db)
        })
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, content_type, body)
    }

    #[tokio::test]
    async fn serves_docs_when_enabled() {
        let (status, content_type, body) = get_docs(true, OPENAPI_PATH).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "application/json");
        let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(spec["info"]["title"], "Billo API");
        assert!(spec["paths"]["/api/v1/accounts/{id}"].is_object());

        let (status, content_type, body) = get_docs(true, DOCS_PATH).await;
        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/html"), "{content_type}");
        let page = String::from_utf8_lossy(&body);
        assert!(page.contains(&format!("url: '{OPENAPI_PATH}'")), "{page}");
        assert!(page.contains("theme: 'elysiajs'"), "{page}");
    }

    #[tokio::test]
    async fn no_docs_when_disabled() {
        for path in [OPENAPI_PATH, DOCS_PATH] {
            let (status, content_type, body) = get_docs(false, path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(content_type, "application/json");
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["error"]["code"], "not_found");
        }
    }

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
