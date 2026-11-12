//! iOS / Android: SQLite (`rusqlite`, bundled) in the app's data directory.
//!
//! Records and outbox entries are stored as JSON next to their keys; the app only
//! ever loads everything at start-up and writes by key, so no columns are needed.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::{Connection, params};

use super::{Backend, LocalRecord, Meta, OutboxEntry, PersistOp, Snapshot, outbox_key, record_key};

pub struct SqliteBackend {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS records (key TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS outbox  (key TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meta    (key TEXT PRIMARY KEY, json TEXT NOT NULL);
";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl SqliteBackend {
    /// `billo/local.db` in the app's data directory.
    pub async fn open() -> Result<Self, String> {
        let dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("billo");
        std::fs::create_dir_all(&dir).map_err(err)?;
        Self::open_at(dir.join("local.db"))
    }

    pub fn open_at(path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(err)?;
        Self::with(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, String> {
        Self::with(Connection::open_in_memory().map_err(err)?)
    }

    fn with(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(SCHEMA).map_err(err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn all<T: serde::de::DeserializeOwned>(conn: &Connection, table: &str) -> Result<Vec<T>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT json FROM {table} ORDER BY key"))
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(err)?;
    let mut out = Vec::new();
    for json in rows {
        // A row this app version can't read is skipped rather than losing the rest.
        if let Ok(v) = serde_json::from_str(&json.map_err(err)?) {
            out.push(v);
        }
    }
    Ok(out)
}

impl Backend for SqliteBackend {
    async fn load(&self) -> Result<Snapshot, String> {
        let conn = self.conn.lock().map_err(err)?;
        let records: Vec<LocalRecord> = all(&conn, "records")?;
        let outbox: Vec<OutboxEntry> = all(&conn, "outbox")?;
        let meta: Option<Meta> = all(&conn, "meta")?.into_iter().next();
        Ok(Snapshot {
            meta: meta.unwrap_or_default(),
            records,
            outbox,
        })
    }

    async fn write(&self, ops: Vec<PersistOp>) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(err)?;
        let tx = conn.transaction().map_err(err)?;
        for op in &ops {
            match op {
                PersistOp::PutRecord(r) => tx.execute(
                    "INSERT OR REPLACE INTO records (key, json) VALUES (?1, ?2)",
                    params![
                        record_key(r.entity, r.id),
                        serde_json::to_string(r).map_err(err)?
                    ],
                ),
                PersistOp::RemoveRecord(e, id) => tx.execute(
                    "DELETE FROM records WHERE key = ?1",
                    params![record_key(*e, *id)],
                ),
                PersistOp::PutOutbox(e) => tx.execute(
                    "INSERT OR REPLACE INTO outbox (key, json) VALUES (?1, ?2)",
                    params![outbox_key(e.seq), serde_json::to_string(e).map_err(err)?],
                ),
                PersistOp::RemoveOutbox(seq) => tx.execute(
                    "DELETE FROM outbox WHERE key = ?1",
                    params![outbox_key(*seq)],
                ),
                PersistOp::SetMeta(m) => tx.execute(
                    "INSERT OR REPLACE INTO meta (key, json) VALUES ('meta', ?1)",
                    params![serde_json::to_string(m).map_err(err)?],
                ),
                PersistOp::Clear => tx
                    .execute_batch("DELETE FROM records; DELETE FROM outbox; DELETE FROM meta;")
                    .map(|()| 0),
            }
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use billo_api::sync::{Entity, Mutation, MutationOp};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::local::{LocalState, block_on};

    #[test]
    fn round_trips_and_applies_ops_in_order() {
        let db = SqliteBackend::in_memory().unwrap();
        assert_eq!(block_on(db.load()).unwrap(), Snapshot::default());
        let mut state = LocalState::default();
        let mut ops = state.set_user(Uuid::now_v7());
        let a = Uuid::now_v7();
        let fields = json!({ "kind": "cash", "name": "A", "currency": "THB" });
        let now = time::OffsetDateTime::UNIX_EPOCH;
        ops.extend(
            state
                .put(Entity::Account, a, fields.clone(), Uuid::now_v7(), now)
                .1,
        );
        ops.extend(
            state
                .put(Entity::Account, Uuid::now_v7(), fields, Uuid::now_v7(), now)
                .1,
        );
        ops.extend(state.delete(Entity::Account, a, Uuid::now_v7()).unwrap());
        block_on(db.write(ops)).unwrap();
        let snap = block_on(db.load()).unwrap();
        assert_eq!(snap, state.snapshot());
        assert_eq!(snap.outbox.len(), 3);
        assert!(matches!(
            snap.outbox[2].mutation,
            Mutation {
                op: MutationOp::Delete,
                ..
            }
        ));

        let first = snap.outbox[0].seq;
        block_on(db.write(vec![
            PersistOp::RemoveOutbox(first),
            PersistOp::RemoveRecord(Entity::Account, a),
        ]))
        .unwrap();
        let loaded = block_on(db.load()).unwrap();
        assert_eq!(loaded.outbox.len(), 2);
        assert_eq!(loaded.records.len(), 1);

        block_on(db.write(vec![PersistOp::Clear])).unwrap();
        assert_eq!(block_on(db.load()).unwrap(), Snapshot::default());
    }

    #[test]
    fn persists_to_a_file() {
        let path = std::env::temp_dir().join(format!("billo-test-{}.db", Uuid::now_v7()));
        {
            let db = SqliteBackend::open_at(path.clone()).unwrap();
            let mut state = LocalState::default();
            block_on(db.write(state.set_user(Uuid::nil()))).unwrap();
        }
        let db = SqliteBackend::open_at(path.clone()).unwrap();
        assert_eq!(block_on(db.load()).unwrap().meta.user_id, Some(Uuid::nil()));
        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
