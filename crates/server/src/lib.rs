//! Billo server: Axum + Postgres.

pub mod app;
pub mod config;
pub mod routes;

/// Migrations from `/migrations`, embedded at compile time.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
