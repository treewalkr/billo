//! The CRUD resources: table mapping and validation for each, on top of [`crate::repo`].

pub mod accounts;
pub mod budgets;
pub mod categories;
pub mod installment_plans;
pub mod transactions;

use sqlx::Row;
use sqlx::postgres::PgRow;

/// Text enums stored in `TEXT` columns with a `CHECK`, (de)serialised via serde's
/// snake_case names so the database and the API use the same strings.
pub fn enum_to_db<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("unit enum variants serialise to strings"),
    }
}

pub fn enum_from_db<T: serde::de::DeserializeOwned>(s: String) -> sqlx::Result<T> {
    serde_json::from_value(serde_json::Value::String(s))
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

/// A `{minor, currency}` pair from two columns.
pub fn money_from_row(row: &PgRow, minor: &str, currency: &str) -> sqlx::Result<billo_api::Money> {
    Ok(billo_api::Money {
        minor: row.try_get(minor)?,
        currency: row.try_get(currency)?,
    })
}
