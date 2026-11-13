//! Billo server: Axum + Postgres.

pub mod app;
pub mod auth;
pub mod config;
pub mod error;
pub mod extract;
pub mod google;
pub mod repo;
pub mod resources;
pub mod routes;
pub mod validate;

/// Migrations from `/migrations`, embedded at compile time.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
