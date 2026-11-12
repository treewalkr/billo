//! Configuration from environment variables.

use std::fmt;
use std::net::SocketAddr;

use axum::http::HeaderValue;

pub const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    /// Browser origins allowed to call the API (dev only, e.g. `dx serve`).
    /// Empty means no CORS layer.
    pub cors_allowed_origins: Vec<HeaderValue>,
    /// `DEV_LOGIN=true` enables password-less sign-in by email. Development only.
    pub dev_login: bool,
    /// Google sign-in; `None` unless `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are set.
    pub google: Option<GoogleConfig>,
}

pub const DEFAULT_PUBLIC_URL: &str = "http://localhost:8080";
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
pub const GOOGLE_ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// Google OAuth settings. `Debug` never prints the client secret.
#[derive(Clone, PartialEq, Eq)]
pub struct GoogleConfig {
    /// The web OAuth client: `aud` of web-flow ID tokens.
    pub client_id: String,
    pub client_secret: String,
    /// The iOS OAuth client, also accepted as `aud` by `POST /auth/google`.
    pub ios_client_id: Option<String>,
    /// `{PUBLIC_URL}/api/v1/auth/google/callback`; must be registered with Google.
    pub redirect_uri: String,
    /// App origins (`scheme://host[:port]`) the web flow may return to.
    pub app_origins: Vec<String>,
    pub auth_url: String,
    pub token_url: String,
    pub jwks_url: String,
    /// Accepted `iss` values.
    pub issuers: Vec<String>,
}

impl fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("ios_client_id", &self.ios_client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("app_origins", &self.app_origins)
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("jwks_url", &self.jwks_url)
            .field("issuers", &self.issuers)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Missing(&'static str),
    InvalidBindAddr(String),
    InvalidCorsOrigin(String),
    InvalidUrl(&'static str, String),
    InvalidAppOrigin(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "environment variable {name} is required"),
            Self::InvalidBindAddr(v) => write!(f, "BIND_ADDR is not a socket address: {v}"),
            Self::InvalidCorsOrigin(v) => {
                write!(f, "CORS_ALLOWED_ORIGINS has an invalid origin: {v}")
            }
            Self::InvalidUrl(name, v) => write!(f, "{name} is not an http(s) URL: {v}"),
            Self::InvalidAppOrigin(v) => write!(
                f,
                "APP_REDIRECT_ORIGINS has an invalid origin (want scheme://host[:port]): {v}"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Reads the process environment (after `.env`, if any, has been loaded).
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let database_url = get("DATABASE_URL")
            .filter(|v| !v.is_empty())
            .ok_or(ConfigError::Missing("DATABASE_URL"))?;
        let bind = get("BIND_ADDR")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned());
        let bind_addr = bind
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddr(bind))?;
        let cors_allowed_origins = parse_origins(&get("CORS_ALLOWED_ORIGINS").unwrap_or_default())?;
        let dev_login = get("DEV_LOGIN").is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
        let google = google_from_lookup(&get)?;
        Ok(Self {
            database_url,
            bind_addr,
            cors_allowed_origins,
            dev_login,
            google,
        })
    }
}

fn google_from_lookup(
    get: &impl Fn(&str) -> Option<String>,
) -> Result<Option<GoogleConfig>, ConfigError> {
    let var = |k: &str| {
        get(k)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let (Some(client_id), Some(client_secret)) =
        (var("GOOGLE_CLIENT_ID"), var("GOOGLE_CLIENT_SECRET"))
    else {
        return Ok(None);
    };
    let url = |name: &'static str, default: &str| -> Result<String, ConfigError> {
        let v = var(name).unwrap_or_else(|| default.to_owned());
        match url::Url::parse(&v) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => {
                Ok(v.trim_end_matches('/').to_owned())
            }
            _ => Err(ConfigError::InvalidUrl(name, v)),
        }
    };
    let public_url = url("PUBLIC_URL", DEFAULT_PUBLIC_URL)?;
    let app_origins = var("APP_REDIRECT_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map(|o| app_origin(o).ok_or_else(|| ConfigError::InvalidAppOrigin(o.to_owned())))
        .collect::<Result<_, _>>()?;
    let issuers = match var("GOOGLE_ISSUER") {
        Some(v) => v
            .split(',')
            .map(str::trim)
            .filter(|i| !i.is_empty())
            .map(str::to_owned)
            .collect(),
        None => GOOGLE_ISSUERS.map(str::to_owned).to_vec(),
    };
    Ok(Some(GoogleConfig {
        client_id,
        client_secret,
        ios_client_id: var("GOOGLE_IOS_CLIENT_ID"),
        redirect_uri: format!("{public_url}/api/v1/auth/google/callback"),
        app_origins,
        auth_url: url("GOOGLE_AUTH_URL", GOOGLE_AUTH_URL)?,
        token_url: url("GOOGLE_TOKEN_URL", GOOGLE_TOKEN_URL)?,
        jwks_url: url("GOOGLE_JWKS_URL", GOOGLE_JWKS_URL)?,
        issuers,
    }))
}

/// `raw` as a bare origin (`http://localhost:8081`), or `None` if it has a path,
/// query, credentials or a non-http(s) scheme.
pub fn app_origin(raw: &str) -> Option<String> {
    let u = url::Url::parse(raw).ok()?;
    let origin = u.origin().ascii_serialization();
    let bare = matches!(u.scheme(), "http" | "https")
        && u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none();
    bare.then_some(origin)
}

/// Comma-separated origins, e.g. `http://localhost:8081,http://127.0.0.1:8081`.
fn parse_origins(raw: &str) -> Result<Vec<HeaderValue>, ConfigError> {
    raw.split(',')
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .map(|o| {
            if !(o.starts_with("http://") || o.starts_with("https://")) {
                return Err(ConfigError::InvalidCorsOrigin(o.to_owned()));
            }
            HeaderValue::from_str(o).map_err(|_| ConfigError::InvalidCorsOrigin(o.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            vars.iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn defaults_bind_addr() {
        let c = Config::from_lookup(lookup(&[("DATABASE_URL", "postgres://x")])).unwrap();
        assert_eq!(c.database_url, "postgres://x");
        assert_eq!(c.bind_addr, "0.0.0.0:8080".parse().unwrap());
        assert!(c.cors_allowed_origins.is_empty());
        assert!(!c.dev_login);
        assert!(c.google.is_none());
    }

    #[test]
    fn google_needs_id_and_secret() {
        let only_id = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "x"),
            ("GOOGLE_CLIENT_ID", "web.apps"),
            ("GOOGLE_CLIENT_SECRET", " "),
        ]))
        .unwrap();
        assert!(only_id.google.is_none());

        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "x"),
            ("GOOGLE_CLIENT_ID", "web.apps"),
            ("GOOGLE_CLIENT_SECRET", "s3cret"),
            (
                "APP_REDIRECT_ORIGINS",
                "http://localhost:8081, https://app.example/",
            ),
        ]))
        .unwrap();
        let g = c.google.unwrap();
        assert_eq!(g.client_id, "web.apps");
        assert_eq!(g.ios_client_id, None);
        assert_eq!(
            g.redirect_uri,
            "http://localhost:8080/api/v1/auth/google/callback"
        );
        assert_eq!(
            g.app_origins,
            ["http://localhost:8081", "https://app.example"]
        );
        assert_eq!(g.auth_url, GOOGLE_AUTH_URL);
        assert_eq!(g.token_url, GOOGLE_TOKEN_URL);
        assert_eq!(g.jwks_url, GOOGLE_JWKS_URL);
        assert_eq!(g.issuers, GOOGLE_ISSUERS);
        let debug = format!("{g:?}");
        assert!(!debug.contains("s3cret"), "{debug}");
    }

    #[test]
    fn google_public_url_and_endpoints() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "x"),
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "secret"),
            ("GOOGLE_IOS_CLIENT_ID", "ios.apps"),
            ("PUBLIC_URL", "https://billo.example/"),
            ("GOOGLE_TOKEN_URL", "http://127.0.0.1:9/token"),
            ("GOOGLE_ISSUER", "http://mock"),
        ]))
        .unwrap();
        let g = c.google.unwrap();
        assert_eq!(
            g.redirect_uri,
            "https://billo.example/api/v1/auth/google/callback"
        );
        assert_eq!(g.ios_client_id.as_deref(), Some("ios.apps"));
        assert_eq!(g.token_url, "http://127.0.0.1:9/token");
        assert_eq!(g.issuers, ["http://mock"]);
    }

    #[test]
    fn rejects_bad_google_settings() {
        let base = [
            ("DATABASE_URL", "x"),
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "secret"),
        ];
        let with = |extra: (&'static str, &'static str)| {
            let mut v = base.to_vec();
            v.push(extra);
            Config::from_lookup(lookup(&v)).unwrap_err()
        };
        assert_eq!(
            with(("PUBLIC_URL", "localhost:8080")),
            ConfigError::InvalidUrl("PUBLIC_URL", "localhost:8080".into())
        );
        for bad in ["http://localhost:8081/app", "localhost:8081", "ftp://x"] {
            assert_eq!(
                with(("APP_REDIRECT_ORIGINS", bad)),
                ConfigError::InvalidAppOrigin(bad.into())
            );
        }
    }

    #[test]
    fn dev_login_only_when_true() {
        for (v, want) in [
            ("true", true),
            ("TRUE", true),
            ("1", false),
            ("yes", false),
            ("", false),
        ] {
            let c =
                Config::from_lookup(lookup(&[("DATABASE_URL", "x"), ("DEV_LOGIN", v)])).unwrap();
            assert_eq!(c.dev_login, want, "DEV_LOGIN={v:?}");
        }
    }

    #[test]
    fn reads_cors_origins() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            (
                "CORS_ALLOWED_ORIGINS",
                " http://localhost:8081, ,http://127.0.0.1:8081 ",
            ),
        ]))
        .unwrap();
        assert_eq!(
            c.cors_allowed_origins,
            vec![
                HeaderValue::from_static("http://localhost:8081"),
                HeaderValue::from_static("http://127.0.0.1:8081"),
            ]
        );
    }

    #[test]
    fn rejects_bad_cors_origin() {
        assert_eq!(
            Config::from_lookup(lookup(&[
                ("DATABASE_URL", "x"),
                ("CORS_ALLOWED_ORIGINS", "localhost:8081"),
            ]))
            .unwrap_err(),
            ConfigError::InvalidCorsOrigin("localhost:8081".into())
        );
    }

    #[test]
    fn reads_bind_addr() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("BIND_ADDR", "127.0.0.1:3000"),
        ]))
        .unwrap();
        assert_eq!(c.bind_addr, "127.0.0.1:3000".parse().unwrap());
    }

    #[test]
    fn requires_database_url() {
        assert_eq!(
            Config::from_lookup(lookup(&[])).unwrap_err(),
            ConfigError::Missing("DATABASE_URL")
        );
    }

    #[test]
    fn rejects_bad_bind_addr() {
        assert_eq!(
            Config::from_lookup(lookup(&[("DATABASE_URL", "x"), ("BIND_ADDR", "nope")]))
                .unwrap_err(),
            ConfigError::InvalidBindAddr("nope".into())
        );
    }
}
