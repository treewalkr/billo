//! Google sign-in against a mock Google (token + JWKS endpoints on a random port),
//! with ID tokens signed by the TEST-ONLY keys in `tests/fixtures/`.
//! Skipped when `DATABASE_URL` is unset.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::routing::{get, post};
use billo_server::app::{AppState, router};
use billo_server::auth::hash_token;
use billo_server::config::{GOOGLE_ISSUERS, GoogleConfig};
use billo_server::google::{Google, code_challenge};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

const WEB_CLIENT: &str = "web-client.test";
const IOS_CLIENT: &str = "ios-client.test";
const SECRET: &str = "test-secret";
const APP: &str = "http://localhost:8081";
const KEY_A: &[u8] = include_bytes!("fixtures/test-rsa-a.pem");
const KEY_B: &[u8] = include_bytes!("fixtures/test-rsa-b.pem");
const JWK_A: &str = include_str!("fixtures/test-rsa-a.jwk.json");
const JWK_B: &str = include_str!("fixtures/test-rsa-b.jwk.json");

// ---------------------------------------------------------------------------------
// Mock Google

#[derive(Default)]
struct MockState {
    /// code → (id_token, expected PKCE challenge)
    codes: HashMap<String, (String, String)>,
    /// Published keys: (kid, jwk JSON)
    keys: Vec<(&'static str, &'static str)>,
    jwks_fetches: usize,
}

type Shared = Arc<Mutex<MockState>>;

async fn mock_token(
    State(m): State<Shared>,
    Form(f): Form<HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let field = |k: &str| f.get(k).map(String::as_str).unwrap_or("");
    let bad = || {
        (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_grant"})),
        )
            .into_response()
    };
    if field("grant_type") != "authorization_code"
        || field("client_id") != WEB_CLIENT
        || field("client_secret") != SECRET
        || field("redirect_uri") != "http://localhost:8080/api/v1/auth/google/callback"
    {
        return bad();
    }
    let Some((id_token, challenge)) = m.lock().unwrap().codes.remove(field("code")) else {
        return bad();
    };
    if code_challenge(field("code_verifier")) != challenge {
        return bad();
    }
    axum::Json(json!({ "access_token": "x", "id_token": id_token, "token_type": "Bearer" }))
        .into_response()
}

async fn mock_jwks(State(m): State<Shared>) -> axum::Json<Value> {
    let mut m = m.lock().unwrap();
    m.jwks_fetches += 1;
    let keys: Vec<Value> = m
        .keys
        .iter()
        .map(|(kid, jwk)| {
            let mut v: Value = serde_json::from_str(jwk).unwrap();
            v["kid"] = json!(kid);
            v["kty"] = json!("RSA");
            v["alg"] = json!("RS256");
            v["use"] = json!("sig");
            v
        })
        .collect();
    axum::Json(json!({ "keys": keys }))
}

struct T {
    app: Router,
    db: PgPool,
    mock: Shared,
}

async fn setup() -> Option<T> {
    let url = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty());
    let Some(url) = url else {
        eprintln!("skipping: DATABASE_URL is not set");
        return None;
    };
    let db = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();

    let mock: Shared = Arc::new(Mutex::new(MockState {
        keys: vec![("key-a", JWK_A)],
        ..Default::default()
    }));
    let mock_app = Router::new()
        .route("/token", post(mock_token))
        .route("/jwks", get(mock_jwks))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

    let mut google = Google::new(GoogleConfig {
        client_id: WEB_CLIENT.into(),
        client_secret: SECRET.into(),
        ios_client_id: Some(IOS_CLIENT.into()),
        redirect_uri: "http://localhost:8080/api/v1/auth/google/callback".into(),
        app_origins: vec![APP.into()],
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: format!("{base}/token"),
        jwks_url: format!("{base}/jwks"),
        issuers: GOOGLE_ISSUERS.map(str::to_owned).to_vec(),
    })
    .unwrap();
    google.min_refetch = Duration::ZERO;
    let app = router(AppState {
        db: db.clone(),
        dev_login: false,
        google: Some(Arc::new(google)),
    });
    Some(T { app, db, mock })
}

// ---------------------------------------------------------------------------------
// Helpers

struct Res {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Res {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap())
            .unwrap_or_else(|| panic!("no Location ({}): {}", self.status, self.body))
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

impl T {
    async fn call(
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
                Body::from(v.to_string())
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
        Res {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&self, uri: &str) -> Res {
        self.call(Method::GET, uri, None, None).await
    }

    /// `GET /start`; returns Google's query parameters.
    async fn start(&self) -> HashMap<String, String> {
        let res = self
            .get(&format!("/api/v1/auth/google/start?return_to={}", enc(APP)))
            .await;
        assert_eq!(res.status, StatusCode::FOUND, "{}", res.body);
        let u = url::Url::parse(res.location()).unwrap();
        assert_eq!(
            format!("{}://{}{}", u.scheme(), u.host_str().unwrap(), u.path()),
            "https://accounts.google.com/o/oauth2/v2/auth"
        );
        u.query_pairs().into_owned().collect()
    }

    /// Runs start → Google → callback with the ID token `make` builds from the nonce.
    /// Returns the callback response.
    async fn flow(&self, make: impl FnOnce(&str) -> String) -> Res {
        let q = self.start().await;
        let code = format!("code-{}", Uuid::now_v7());
        let id_token = make(&q["nonce"]);
        self.mock
            .lock()
            .unwrap()
            .codes
            .insert(code.clone(), (id_token, q["code_challenge"].clone()));
        self.get(&format!(
            "/api/v1/auth/google/callback?code={}&state={}",
            enc(&code),
            enc(&q["state"])
        ))
        .await
    }

    async fn user_email(&self, user_id: &str) -> String {
        let (email,): (String,) = sqlx::query_as("SELECT email FROM users WHERE id = $1")
            .bind(user_id.parse::<Uuid>().unwrap())
            .fetch_one(&self.db)
            .await
            .unwrap();
        email
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// Valid claims for `sub`, as Google would issue them to the web client.
fn claims(sub: &str, email: &str, nonce: Option<&str>) -> Value {
    let mut c = json!({
        "iss": "https://accounts.google.com",
        "azp": WEB_CLIENT,
        "aud": WEB_CLIENT,
        "sub": sub,
        "email": email,
        "email_verified": true,
        "iat": now(),
        "exp": now() + 3600,
    });
    if let Some(n) = nonce {
        c["nonce"] = json!(n);
    }
    c
}

fn sign(claims: &Value, pem: &[u8], kid: &str) -> String {
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some(kid.into());
    encode(&h, claims, &EncodingKey::from_rsa_pem(pem).unwrap()).unwrap()
}

fn sub() -> String {
    format!("g-{}", Uuid::now_v7())
}

fn fragment(location: &str) -> HashMap<String, String> {
    let u = url::Url::parse(location).unwrap();
    assert_eq!(u.origin().ascii_serialization(), APP, "{location}");
    url::form_urlencoded::parse(u.fragment().unwrap_or("").as_bytes())
        .into_owned()
        .collect()
}

fn error_of(res: &Res) -> String {
    assert_eq!(res.status, StatusCode::FOUND, "{}", res.body);
    fragment(res.location())
        .remove("billo_error")
        .unwrap_or_else(|| panic!("no billo_error in {}", res.location()))
}

/// Flow with claims tweaked by `edit`, signed by key A.
async fn flow_with(t: &T, edit: impl FnOnce(&mut Value)) -> Res {
    t.flow(|nonce| {
        let mut c = claims(&sub(), "x@example.com", Some(nonce));
        edit(&mut c);
        sign(&c, KEY_A, "key-a")
    })
    .await
}

// ---------------------------------------------------------------------------------
// Web flow

#[tokio::test]
async fn start_redirects_to_google_with_pkce_state_and_nonce() {
    let Some(t) = setup().await else { return };
    let q = t.start().await;
    assert_eq!(q["client_id"], WEB_CLIENT);
    assert_eq!(
        q["redirect_uri"],
        "http://localhost:8080/api/v1/auth/google/callback"
    );
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["scope"], "openid email profile");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["prompt"], "select_account");
    assert_eq!(q["code_challenge"].len(), 43);
    assert!(q["state"].len() >= 43 && q["nonce"].len() >= 43);
    assert_ne!(q["state"], q["nonce"]);
    // The attempt is stored by state hash, expiring in ~10 minutes.
    let (mins,): (f64,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM expires_at - now())::float8 / 60 FROM login_attempts WHERE state_hash = $1",
    )
    .bind(&hash_token(&q["state"])[..])
    .fetch_one(&t.db)
    .await
    .unwrap();
    assert!((9.0..=10.0).contains(&mins), "{mins}");
}

#[tokio::test]
async fn happy_path_signs_in_with_fragment_token() {
    let Some(t) = setup().await else { return };
    let sub = sub();
    let email = format!("{sub}@Example.com");
    let res = t
        .flow(|nonce| sign(&claims(&sub, &email, Some(nonce)), KEY_A, "key-a"))
        .await;
    assert_eq!(res.status, StatusCode::FOUND, "{}", res.body);
    assert_eq!(res.headers[header::CACHE_CONTROL], "no-store");
    let loc = res.location();
    assert!(
        loc.starts_with("http://localhost:8081/#billo_token="),
        "{loc}"
    );
    let f = fragment(loc);
    let token = &f["billo_token"];
    assert_eq!(f["email"], email.to_lowercase());

    let me = t.call(Method::GET, "/api/v1/me", Some(token), None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.json()["user_id"], f["user_id"].as_str());
    let (stored_sub,): (String,) = sqlx::query_as("SELECT google_sub FROM users WHERE id = $1")
        .bind(f["user_id"].parse::<Uuid>().unwrap())
        .fetch_one(&t.db)
        .await
        .unwrap();
    assert_eq!(stored_sub, sub);
}

#[tokio::test]
async fn return_to_keeps_path_and_drops_old_fragment() {
    let Some(t) = setup().await else { return };
    let res = t
        .get(&format!(
            "/api/v1/auth/google/start?return_to={}",
            enc("http://localhost:8081/app/?x=1#old")
        ))
        .await;
    let state = url::Url::parse(res.location())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let cb = t
        .get(&format!(
            "/api/v1/auth/google/callback?error=access_denied&state={}",
            enc(&state)
        ))
        .await;
    assert_eq!(
        cb.location(),
        "http://localhost:8081/app/?x=1#billo_error=cancelled"
    );
}

#[tokio::test]
async fn disallowed_return_to_is_rejected() {
    let Some(t) = setup().await else { return };
    for bad in [
        None,
        Some("http://evil.example"),
        Some("http://localhost:8081.evil.example"),
        Some("https://localhost:8081"),
        Some("http://localhost:8082"),
        Some("http://user@localhost:8081"),
        Some("javascript:alert(1)"),
        Some("/relative"),
    ] {
        let uri = match bad {
            Some(r) => format!("/api/v1/auth/google/start?return_to={}", enc(r)),
            None => "/api/v1/auth/google/start".to_owned(),
        };
        let res = t.get(&uri).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{bad:?}");
        assert!(res.headers.get(header::LOCATION).is_none());
        assert_eq!(res.json()["error"]["code"], "validation");
    }
}

#[tokio::test]
async fn unknown_state_shows_error_page() {
    let Some(t) = setup().await else { return };
    for uri in [
        "/api/v1/auth/google/callback?code=abc&state=not-a-real-state",
        "/api/v1/auth/google/callback?code=abc",
    ] {
        let res = t.get(uri).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST);
        assert!(res.headers.get(header::LOCATION).is_none());
        assert!(
            res.body.contains("expired or was already used"),
            "{}",
            res.body
        );
    }
}

#[tokio::test]
async fn state_is_single_use() {
    let Some(t) = setup().await else { return };
    let q = t.start().await;
    let code = format!("code-{}", Uuid::now_v7());
    let id_token = sign(
        &claims(&sub(), "x@example.com", Some(&q["nonce"])),
        KEY_A,
        "key-a",
    );
    t.mock
        .lock()
        .unwrap()
        .codes
        .insert(code.clone(), (id_token, q["code_challenge"].clone()));
    let uri = format!(
        "/api/v1/auth/google/callback?code={}&state={}",
        enc(&code),
        enc(&q["state"])
    );
    let first = t.get(&uri).await;
    assert!(fragment(first.location()).contains_key("billo_token"));
    let again = t.get(&uri).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    assert!(again.headers.get(header::LOCATION).is_none());
}

#[tokio::test]
async fn expired_attempt_is_rejected() {
    let Some(t) = setup().await else { return };
    let q = t.start().await;
    sqlx::query(
        "UPDATE login_attempts SET expires_at = now() - interval '1 second' WHERE state_hash = $1",
    )
    .bind(&hash_token(&q["state"])[..])
    .execute(&t.db)
    .await
    .unwrap();
    let res = t
        .get(&format!(
            "/api/v1/auth/google/callback?code=abc&state={}",
            enc(&q["state"])
        ))
        .await;
    assert_eq!(error_of(&res), "expired");
    // Consumed even though it had expired.
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM login_attempts WHERE state_hash = $1")
        .bind(&hash_token(&q["state"])[..])
        .fetch_one(&t.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn failed_code_exchange_is_reported() {
    let Some(t) = setup().await else { return };
    let q = t.start().await;
    // The mock doesn't know this code.
    let res = t
        .get(&format!(
            "/api/v1/auth/google/callback?code=unknown&state={}",
            enc(&q["state"])
        ))
        .await;
    assert_eq!(error_of(&res), "google");
}

#[tokio::test]
async fn wrong_audience_is_rejected() {
    let Some(t) = setup().await else { return };
    let res = flow_with(&t, |c| c["aud"] = json!("someone-else.apps")).await;
    assert_eq!(error_of(&res), "invalid_token");
    // The iOS client is only accepted by the native endpoint, not the web flow.
    let res = flow_with(&t, |c| c["aud"] = json!(IOS_CLIENT)).await;
    assert_eq!(error_of(&res), "invalid_token");
}

#[tokio::test]
async fn wrong_issuer_is_rejected() {
    let Some(t) = setup().await else { return };
    let res = flow_with(&t, |c| c["iss"] = json!("https://evil.example")).await;
    assert_eq!(error_of(&res), "invalid_token");
    // Both of Google's spellings are accepted.
    let res = flow_with(&t, |c| c["iss"] = json!("accounts.google.com")).await;
    assert!(fragment(res.location()).contains_key("billo_token"));
}

#[tokio::test]
async fn expired_or_future_token_is_rejected() {
    let Some(t) = setup().await else { return };
    let res = flow_with(&t, |c| {
        c["iat"] = json!(now() - 7200);
        c["exp"] = json!(now() - 3600);
    })
    .await;
    assert_eq!(error_of(&res), "invalid_token");
    let res = flow_with(&t, |c| c["iat"] = json!(now() + 3600)).await;
    assert_eq!(error_of(&res), "invalid_token");
}

#[tokio::test]
async fn bad_signature_is_rejected() {
    let Some(t) = setup().await else { return };
    // Signed by key B but claiming to be key A.
    let res = t
        .flow(|nonce| {
            sign(
                &claims(&sub(), "x@example.com", Some(nonce)),
                KEY_B,
                "key-a",
            )
        })
        .await;
    assert_eq!(error_of(&res), "invalid_token");
}

#[tokio::test]
async fn unknown_kid_refetches_keys() {
    let Some(t) = setup().await else { return };
    let ok = flow_with(&t, |_| {}).await;
    assert!(fragment(ok.location()).contains_key("billo_token"));
    let fetches = t.mock.lock().unwrap().jwks_fetches;
    assert_eq!(fetches, 1);

    // Cached: no new fetch for a known kid.
    let ok = flow_with(&t, |_| {}).await;
    assert!(fragment(ok.location()).contains_key("billo_token"));
    assert_eq!(t.mock.lock().unwrap().jwks_fetches, 1);

    // Google rotates in key B.
    t.mock.lock().unwrap().keys.push(("key-b", JWK_B));
    let res = t
        .flow(|nonce| {
            sign(
                &claims(&sub(), "x@example.com", Some(nonce)),
                KEY_B,
                "key-b",
            )
        })
        .await;
    assert!(
        fragment(res.location()).contains_key("billo_token"),
        "{}",
        res.location()
    );
    assert_eq!(t.mock.lock().unwrap().jwks_fetches, 2);

    // A kid Google never published is still refused (after a refetch).
    let res = t
        .flow(|nonce| {
            sign(
                &claims(&sub(), "x@example.com", Some(nonce)),
                KEY_B,
                "key-zzz",
            )
        })
        .await;
    assert_eq!(error_of(&res), "invalid_token");
}

#[tokio::test]
async fn nonce_mismatch_is_rejected() {
    let Some(t) = setup().await else { return };
    let res = flow_with(&t, |c| c["nonce"] = json!("some-other-nonce")).await;
    assert_eq!(error_of(&res), "invalid_token");
    let res = flow_with(&t, |c| {
        c.as_object_mut().unwrap().remove("nonce");
    })
    .await;
    assert_eq!(error_of(&res), "invalid_token");
}

#[tokio::test]
async fn unverified_email_is_rejected() {
    let Some(t) = setup().await else { return };
    let res = flow_with(&t, |c| c["email_verified"] = json!(false)).await;
    assert_eq!(error_of(&res), "email_unverified");
    let res = flow_with(&t, |c| {
        c.as_object_mut().unwrap().remove("email_verified");
    })
    .await;
    assert_eq!(error_of(&res), "email_unverified");
}

#[tokio::test]
async fn existing_user_gets_email_updated() {
    let Some(t) = setup().await else { return };
    let sub = sub();
    let first = t
        .flow(|n| sign(&claims(&sub, "old@example.com", Some(n)), KEY_A, "key-a"))
        .await;
    let second = t
        .flow(|n| sign(&claims(&sub, "new@example.com", Some(n)), KEY_A, "key-a"))
        .await;
    let (a, b) = (fragment(first.location()), fragment(second.location()));
    assert_eq!(a["user_id"], b["user_id"]);
    assert_ne!(a["billo_token"], b["billo_token"]);
    assert_eq!(b["email"], "new@example.com");
    assert_eq!(t.user_email(&b["user_id"]).await, "new@example.com");
}

// ---------------------------------------------------------------------------------
// Sign-in methods

#[tokio::test]
async fn methods_report_google_and_dev_login() {
    let Some(t) = setup().await else { return };
    let res = t.get("/api/v1/auth/methods").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        res.json(),
        json!({ "google_sign_in": true, "dev_login": false })
    );

    let Some(plain) = common::app().await else {
        return;
    };
    let res = plain
        .call(Method::GET, "/api/v1/auth/methods", None, None)
        .await;
    assert_eq!(
        res.body,
        json!({ "google_sign_in": false, "dev_login": true })
    );
    let res = plain
        .call(
            Method::GET,
            "/api/v1/auth/google/start?return_to=http://localhost:8081",
            None,
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}
