//! Google sign-in: verifying Google ID tokens against Google's published keys (JWKS),
//! and the pieces of the web OAuth flow (authorization URL, code exchange, PKCE).
//!
//! Verification checks the RS256 signature (key picked by `kid`), `iss`, `aud`,
//! `exp` / `iat` (60 s leeway), the `nonce` when one is expected, and `email_verified`.
//! Keys are cached for an hour; a token signed with an unknown `kid` triggers a
//! refetch (Google rotates keys), at most once per [`Google::min_refetch`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};

use crate::config::GoogleConfig;

/// Leeway for `exp` / `iat`, for clock skew.
pub const LEEWAY_SECS: u64 = 60;
/// How long fetched keys are trusted before a refetch.
const KEYS_TTL: Duration = Duration::from_secs(60 * 60);
/// Google ID tokens are ~1 KB; anything this large isn't one.
const MAX_TOKEN_LEN: usize = 8 * 1024;

/// Who Google says signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleUser {
    pub sub: String,
    pub email: String,
}

/// Why an ID token was refused. The reason is for logs only; it never contains
/// token contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    Invalid(&'static str),
    EmailUnverified,
    /// Google's keys couldn't be fetched.
    Keys(String),
}

#[derive(Default)]
struct KeyCache {
    keys: Arc<HashMap<String, DecodingKey>>,
    fetched: Option<Instant>,
}

pub struct Google {
    pub config: GoogleConfig,
    http: reqwest::Client,
    cache: RwLock<KeyCache>,
    /// One JWKS fetch at a time.
    fetching: Mutex<()>,
    /// Minimum time between refetches caused by an unknown `kid`.
    pub min_refetch: Duration,
}

impl std::fmt::Debug for Google {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Google")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    iat: u64,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<serde_json::Value>,
    #[serde(default)]
    nonce: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

impl Google {
    pub fn new(config: GoogleConfig) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            config,
            http,
            cache: RwLock::new(KeyCache::default()),
            fetching: Mutex::new(()),
            min_refetch: Duration::from_secs(30),
        })
    }

    /// Verifies a Google ID token for one of `audiences`. With `nonce`, the token's
    /// `nonce` claim must match it.
    pub async fn verify(
        &self,
        id_token: &str,
        audiences: &[&str],
        nonce: Option<&str>,
    ) -> Result<GoogleUser, VerifyError> {
        if id_token.len() > MAX_TOKEN_LEN {
            return Err(VerifyError::Invalid("token too large"));
        }
        let header = decode_header(id_token).map_err(|_| VerifyError::Invalid("malformed"))?;
        if header.alg != Algorithm::RS256 {
            return Err(VerifyError::Invalid("algorithm is not RS256"));
        }
        let kid = header.kid.ok_or(VerifyError::Invalid("no kid"))?;
        let key = self.key(&kid).await?;

        let mut v = Validation::new(Algorithm::RS256);
        v.leeway = LEEWAY_SECS;
        v.set_audience(audiences);
        v.set_issuer(&self.config.issuers);
        v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<Claims>(id_token, &key, &v)
            .map_err(|e| {
                use jsonwebtoken::errors::ErrorKind as K;
                VerifyError::Invalid(match e.kind() {
                    K::InvalidSignature => "bad signature",
                    K::ExpiredSignature => "expired",
                    K::InvalidIssuer => "wrong issuer",
                    K::InvalidAudience => "wrong audience",
                    K::MissingRequiredClaim(_) => "missing claim",
                    _ => "invalid",
                })
            })?
            .claims;

        let now = jsonwebtoken::get_current_timestamp();
        if claims.iat > now + LEEWAY_SECS {
            return Err(VerifyError::Invalid("issued in the future"));
        }
        if let Some(want) = nonce
            && claims.nonce.as_deref() != Some(want)
        {
            return Err(VerifyError::Invalid("nonce mismatch"));
        }
        if claims.sub.is_empty() {
            return Err(VerifyError::Invalid("empty sub"));
        }
        // Google sends a boolean; very old tokens used the string "true".
        let verified = matches!(claims.email_verified, Some(serde_json::Value::Bool(true)))
            || matches!(&claims.email_verified, Some(serde_json::Value::String(s)) if s == "true");
        let email = claims
            .email
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty());
        match (verified, email) {
            (true, Some(email)) => Ok(GoogleUser {
                sub: claims.sub,
                email,
            }),
            _ => Err(VerifyError::EmailUnverified),
        }
    }

    /// The key for `kid`, fetching Google's keys when the cache is stale or doesn't
    /// have it.
    async fn key(&self, kid: &str) -> Result<DecodingKey, VerifyError> {
        {
            let c = self.cache.read().await;
            let fresh = c.fetched.is_some_and(|t| t.elapsed() < KEYS_TTL);
            if fresh && let Some(k) = c.keys.get(kid) {
                return Ok(k.clone());
            }
        }
        let _guard = self.fetching.lock().await;
        // Another request may have refreshed the keys while we waited.
        let (stale, recent) = {
            let c = self.cache.read().await;
            if let Some(k) = c.keys.get(kid)
                && c.fetched.is_some_and(|t| t.elapsed() < KEYS_TTL)
            {
                return Ok(k.clone());
            }
            (
                c.fetched.is_none_or(|t| t.elapsed() >= KEYS_TTL),
                c.fetched.is_some_and(|t| t.elapsed() < self.min_refetch),
            )
        };
        if stale || !recent {
            let keys = self.fetch_keys().await?;
            let mut c = self.cache.write().await;
            c.keys = Arc::new(keys);
            c.fetched = Some(Instant::now());
        }
        self.cache
            .read()
            .await
            .keys
            .get(kid)
            .cloned()
            .ok_or(VerifyError::Invalid("unknown kid"))
    }

    async fn fetch_keys(&self) -> Result<HashMap<String, DecodingKey>, VerifyError> {
        let res = self
            .http
            .get(&self.config.jwks_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| VerifyError::Keys(e.without_url().to_string()))?;
        let set: JwkSet = res
            .json()
            .await
            .map_err(|e| VerifyError::Keys(e.without_url().to_string()))?;
        Ok(set
            .keys
            .iter()
            .filter_map(|jwk| {
                let kid = jwk.common.key_id.clone()?;
                Some((kid, DecodingKey::from_jwk(jwk).ok()?))
            })
            .collect())
    }

    /// Google's sign-in page for one login attempt.
    pub fn authorize_url(&self, state: &str, nonce: &str, code_challenge: &str) -> String {
        let c = &self.config;
        let mut u = url::Url::parse(&c.auth_url).expect("auth_url was validated in config");
        u.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &c.client_id)
            .append_pair("redirect_uri", &c.redirect_uri)
            .append_pair("scope", "openid email profile")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("prompt", "select_account");
        u.into()
    }

    /// Trades an authorization code (plus the PKCE verifier and client secret) for
    /// the ID token. Errors are for logs and never include the response body.
    pub async fn exchange_code(&self, code: &str, verifier: &str) -> Result<String, String> {
        let c = &self.config;
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", c.redirect_uri.as_str()),
            ("client_id", c.client_id.as_str()),
            ("client_secret", c.client_secret.as_str()),
            ("code_verifier", verifier),
        ];
        let res = self
            .http
            .post(&c.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let status = res.status();
        if !status.is_success() {
            return Err(format!("token endpoint returned {status}"));
        }
        let body: TokenResponse = res
            .json()
            .await
            .map_err(|_| "token response is not JSON".to_owned())?;
        body.id_token
            .ok_or_else(|| "token response has no id_token".to_owned())
    }
}

/// 32 random bytes, base64url: used for `state`, `nonce` and the PKCE verifier.
pub fn random_string() -> String {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).expect("OS random number generator is unavailable");
    URL_SAFE_NO_PAD.encode(raw)
}

/// PKCE S256: `base64url(sha256(verifier))`.
pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_strings_differ() {
        let a = random_string();
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_string());
    }
}
