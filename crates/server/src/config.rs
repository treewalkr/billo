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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Missing(&'static str),
    InvalidBindAddr(String),
    InvalidCorsOrigin(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "environment variable {name} is required"),
            Self::InvalidBindAddr(v) => write!(f, "BIND_ADDR is not a socket address: {v}"),
            Self::InvalidCorsOrigin(v) => {
                write!(f, "CORS_ALLOWED_ORIGINS has an invalid origin: {v}")
            }
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
        Ok(Self {
            database_url,
            bind_addr,
            cors_allowed_origins,
        })
    }
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
