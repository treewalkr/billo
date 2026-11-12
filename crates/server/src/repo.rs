//! The repository layer: the only code that reads or writes user data. Every query is
//! filtered by the caller's `user_id` (architecture §7), and every write bumps the
//! record's `version` and appends to `change_log` in the same database transaction.

use std::future::Future;

use billo_api::sync::{Entity, EntityState};
use billo_api::{Put, Record};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Postgres, QueryBuilder, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// A table of user-owned, versioned, soft-deletable records.
///
/// Table and column names are `'static` constants, never user input, so building SQL
/// from them is safe.
pub trait Resource: Send + Sync + 'static {
    /// Table name.
    const TABLE: &'static str;
    /// The record type; its name goes in `change_log.entity`.
    const ENTITY_KIND: Entity;
    /// Data columns (everything but `id`, `user_id` and the metadata), in the order
    /// [`Resource::bind`] binds them.
    const COLUMNS: &'static [&'static str];
    /// `ORDER BY` clause for lists.
    const ORDER_BY: &'static str = "created_at, id";

    type Data: Serialize + DeserializeOwned + JsonSchema + PartialEq + Clone + Send + Sync + 'static;
    /// List query parameters.
    type Filter: DeserializeOwned + JsonSchema + Send + Sync + 'static;

    fn data_from_row(row: &PgRow) -> sqlx::Result<Self::Data>;

    /// Binds `data` in [`Resource::COLUMNS`] order.
    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        data: &Self::Data,
    ) -> Query<'q, Postgres, PgArguments>;

    /// Checks client input (through the domain types where they apply) and returns it
    /// normalised. May look up the user's other records through `conn`.
    fn validate<'a>(
        conn: &'a mut PgConnection,
        user_id: Uuid,
        data: Self::Data,
    ) -> impl Future<Output = ApiResult<Self::Data>> + Send + 'a;

    /// Adds `AND ...` conditions for list filters.
    fn filter(_filter: &Self::Filter, _qb: &mut QueryBuilder<Postgres>) {}
}

/// Query parameters for resources without list filters.
#[derive(Debug, serde::Deserialize, JsonSchema)]
pub struct NoFilter {}

fn select_sql<R: Resource>() -> String {
    format!(
        "SELECT id, version, created_at, updated_at, {} FROM {}",
        R::COLUMNS.join(", "),
        R::TABLE
    )
}

fn record_from_row<R: Resource>(row: &PgRow) -> sqlx::Result<Record<R::Data>> {
    Ok(Record {
        id: row.try_get("id")?,
        version: row.try_get("version")?,
        created_at: row.try_get::<OffsetDateTime, _>("created_at")?,
        updated_at: row.try_get::<OffsetDateTime, _>("updated_at")?,
        data: R::data_from_row(row)?,
    })
}

/// Live (not deleted) records of `user_id`.
pub async fn list<R: Resource>(
    db: &PgPool,
    user_id: Uuid,
    filter: &R::Filter,
) -> ApiResult<Vec<Record<R::Data>>> {
    let mut qb = QueryBuilder::new(select_sql::<R>());
    qb.push(" WHERE user_id = ")
        .push_bind(user_id)
        .push(" AND deleted_at IS NULL");
    R::filter(filter, &mut qb);
    qb.push(" ORDER BY ").push(R::ORDER_BY);
    let rows = qb.build().fetch_all(db).await?;
    Ok(rows
        .iter()
        .map(record_from_row::<R>)
        .collect::<sqlx::Result<_>>()?)
}

/// One live record of `user_id`; `NotFound` for other users' ids.
pub async fn get<R: Resource>(db: &PgPool, user_id: Uuid, id: Uuid) -> ApiResult<Record<R::Data>> {
    let sql = format!(
        "{} WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
        select_sql::<R>()
    );
    let row = sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(user_id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(record_from_row::<R>(&row)?)
}

#[derive(Debug)]
pub struct PutOutcome<D> {
    pub record: Record<D>,
    pub created: bool,
    /// `base_version` didn't match the stored version; the write was applied anyway.
    pub conflict: bool,
}

/// Creates or updates record `id` of `user_id`.
///
/// - Unknown id: insert at version 1.
/// - Same user, same data, not deleted: a retry; returns the record unchanged.
/// - Same user otherwise: update (and undelete), version + 1. Last write wins; a
///   `base_version` other than the stored version is reported as a conflict.
/// - Id owned by another user: `Conflict`.
pub async fn put<R: Resource>(
    db: &PgPool,
    user_id: Uuid,
    id: Uuid,
    put: Put<R::Data>,
) -> ApiResult<PutOutcome<R::Data>> {
    let mut tx = db.begin().await?;
    let out = put_in::<R>(&mut tx, user_id, id, put).await?;
    tx.commit().await?;
    Ok(out)
}

/// [`put`] inside the caller's transaction (which the caller commits).
pub async fn put_in<R: Resource>(
    tx: &mut PgConnection,
    user_id: Uuid,
    id: Uuid,
    put: Put<R::Data>,
) -> ApiResult<PutOutcome<R::Data>> {
    let sql = format!(
        "SELECT id, user_id, deleted_at IS NOT NULL AS deleted, version, created_at, updated_at, {} \
         FROM {} WHERE id = $1 FOR UPDATE",
        R::COLUMNS.join(", "),
        R::TABLE
    );
    let existing = sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let existing = match existing {
        None => None,
        Some(row) => {
            let owner: Uuid = row.try_get("user_id")?;
            if owner != user_id {
                return Err(id_taken());
            }
            let deleted: bool = row.try_get("deleted")?;
            Some((record_from_row::<R>(&row)?, deleted))
        }
    };

    let data = R::validate(tx, user_id, put.data).await?;

    if let Some((record, false)) = &existing
        && record.data == data
    {
        return Ok(PutOutcome {
            record: record.clone(),
            created: false,
            conflict: false,
        });
    }
    let conflict = existing
        .as_ref()
        .is_some_and(|(r, _)| put.base_version != Some(r.version));

    let cols = R::COLUMNS;
    let placeholders: Vec<String> = (0..cols.len()).map(|i| format!("${}", i + 3)).collect();
    let updates: Vec<String> = cols.iter().map(|c| format!("{c} = EXCLUDED.{c}")).collect();
    let sql = format!(
        "INSERT INTO {table} AS t (id, user_id, {cols}) VALUES ($1, $2, {placeholders}) \
         ON CONFLICT (id) DO UPDATE SET {updates}, version = t.version + 1, \
         updated_at = now(), deleted_at = NULL \
         WHERE t.user_id = EXCLUDED.user_id \
         RETURNING id, version, created_at, updated_at, {cols}",
        table = R::TABLE,
        cols = cols.join(", "),
        placeholders = placeholders.join(", "),
        updates = updates.join(", "),
    );
    let q = sqlx::query(AssertSqlSafe(sql)).bind(id).bind(user_id);
    // `None`: another user inserted this id concurrently.
    let row = R::bind(q, &data)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(id_taken)?;
    let record = record_from_row::<R>(&row)?;
    let created = record.version == 1;
    log_change::<R>(
        tx,
        user_id,
        id,
        record.version,
        if created { "create" } else { "update" },
    )
    .await?;
    Ok(PutOutcome {
        record,
        created,
        conflict,
    })
}

/// Soft-deletes record `id` of `user_id` (version + 1). Deleting an already deleted
/// record is a no-op, so retries are safe.
pub async fn delete<R: Resource>(db: &PgPool, user_id: Uuid, id: Uuid) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    delete_in::<R>(&mut tx, user_id, id).await?;
    tx.commit().await?;
    Ok(())
}

/// [`delete`] inside the caller's transaction; returns the record's version after it.
pub async fn delete_in<R: Resource>(
    tx: &mut PgConnection,
    user_id: Uuid,
    id: Uuid,
) -> ApiResult<i64> {
    let sql = format!(
        "SELECT deleted_at IS NOT NULL, version FROM {} WHERE id = $1 AND user_id = $2 FOR UPDATE",
        R::TABLE
    );
    let (deleted, version): (bool, i64) = sqlx::query_as(AssertSqlSafe(sql))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    if deleted {
        return Ok(version);
    }
    let sql = format!(
        "UPDATE {} SET deleted_at = now(), updated_at = now(), version = version + 1 \
         WHERE id = $1 AND user_id = $2 RETURNING version",
        R::TABLE
    );
    let (version,): (i64,) = sqlx::query_as(AssertSqlSafe(sql))
        .bind(id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    log_change::<R>(tx, user_id, id, version, "delete").await?;
    Ok(version)
}

/// The current state of records `ids` of `user_id`, deleted ones as tombstones. Ids the
/// user doesn't own are left out.
pub async fn states<R: Resource>(
    conn: &mut PgConnection,
    user_id: Uuid,
    ids: &[Uuid],
) -> ApiResult<Vec<EntityState>> {
    let sql = format!(
        "SELECT id, deleted_at IS NOT NULL AS deleted, version, created_at, updated_at, {} \
         FROM {} WHERE user_id = $1 AND id = ANY($2)",
        R::COLUMNS.join(", "),
        R::TABLE
    );
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(user_id)
        .bind(ids)
        .fetch_all(conn)
        .await?;
    rows.iter()
        .map(|row| {
            let deleted: bool = row.try_get("deleted")?;
            let id: Uuid = row.try_get("id")?;
            let version: i64 = row.try_get("version")?;
            let record = if deleted {
                None
            } else {
                Some(record_json(&record_from_row::<R>(row)?))
            };
            Ok(EntityState {
                entity: R::ENTITY_KIND,
                id,
                version,
                deleted,
                record,
            })
        })
        .collect()
}

/// A record as the JSON object the API returns.
pub fn record_json<D: Serialize>(record: &Record<D>) -> serde_json::Value {
    serde_json::to_value(record).unwrap_or(serde_json::Value::Null)
}

fn id_taken() -> ApiError {
    ApiError::Conflict("this id is already used by another record".into())
}

async fn log_change<R: Resource>(
    conn: &mut PgConnection,
    user_id: Uuid,
    entity_id: Uuid,
    version: i64,
    op: &'static str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO change_log (user_id, entity, entity_id, version, op) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(R::ENTITY_KIND.as_str())
    .bind(entity_id)
    .bind(version)
    .bind(op)
    .execute(conn)
    .await?;
    Ok(())
}

/// Whether `user_id` owns record `id` in `table` (deleted or not), with selected
/// columns. For validating references between resources.
pub async fn owned_row(
    conn: &mut PgConnection,
    table: &'static str,
    columns: &'static str,
    user_id: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<PgRow>> {
    let sql = format!("SELECT {columns} FROM {table} WHERE id = $1 AND user_id = $2");
    sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(user_id)
        .fetch_optional(conn)
        .await
}
