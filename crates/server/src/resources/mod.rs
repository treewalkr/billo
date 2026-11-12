//! The CRUD resources: table mapping and validation for each, on top of [`crate::repo`].

pub mod accounts;
pub mod categories;

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
