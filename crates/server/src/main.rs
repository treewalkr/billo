use std::sync::Arc;

use billo_server::app::{AppState, router, with_cors};
use billo_server::config::Config;
use billo_server::google::Google;
use billo_server::routes::health::{VERSION, build_number};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `.env` is a dev convenience; it's fine if it doesn't exist.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    billo_server::MIGRATOR.run(&db).await?;

    if config.dev_login {
        tracing::warn!(
            "DEV_LOGIN=true: anyone can sign in as any email via POST /api/v1/auth/dev-login. \
             NEVER enable this in production."
        );
    }
    let google = match config.google.clone() {
        Some(g) => {
            if g.app_origins.is_empty() {
                tracing::warn!(
                    "Google sign-in is on but APP_REDIRECT_ORIGINS is empty: \
                     the web sign-in flow will refuse every return_to"
                );
            }
            tracing::info!(
                redirect_uri = %g.redirect_uri,
                ios = g.ios_client_id.is_some(),
                "Google sign-in enabled"
            );
            Some(Arc::new(Google::new(g)?))
        }
        None => {
            tracing::info!(
                "Google sign-in disabled (GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET unset)"
            );
            None
        }
    };
    if config.api_docs {
        tracing::info!("API_DOCS=true: API reference at /api/docs, spec at /api/openapi.json");
    }
    let state = AppState {
        db,
        dev_login: config.dev_login,
        google,
        api_docs: config.api_docs,
    };
    let app = with_cors(router(state), &config.cors_allowed_origins);
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(
        addr = %config.bind_addr,
        version = VERSION,
        build = build_number(),
        "billo-server listening"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("billo-server stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for Ctrl-C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
