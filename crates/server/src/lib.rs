//! Billo server: Axum + Postgres.

pub mod app;
pub mod auth;
pub mod config;
pub mod error;
pub mod extract;
pub mod routes;

/// Migrations from `/migrations`, embedded at compile time.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
