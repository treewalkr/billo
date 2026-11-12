//! Web: IndexedDB through the `idb` crate (a small, maintained futures wrapper over
//! `web-sys`; `rexie` is a thinner layer on top of the same crate).
//!
//! Three object stores with out-of-line string keys and JSON string values: `records`
//! (`entity:id`), `outbox` (zero-padded seq, so keys sort in order) and `meta`.

use idb::{Database, ObjectStore, TransactionMode};
use wasm_bindgen::JsValue;

use super::{Backend, LocalRecord, Meta, OutboxEntry, PersistOp, Snapshot, outbox_key, record_key};

const DB_NAME: &str = "billo";
const DB_VERSION: u32 = 1;
const STORES: [&str; 3] = ["records", "outbox", "meta"];

pub struct IdbBackend {
    db: Database,
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

impl IdbBackend {
    pub async fn open() -> Result<Self, String> {
        let mut builder = Database::builder(DB_NAME).version(DB_VERSION);
        for name in STORES {
            builder = builder.add_object_store(ObjectStore::builder(name));
        }
        Ok(Self {
            db: builder.build().await.map_err(err)?,
        })
    }
}

async fn all<T: serde::de::DeserializeOwned>(store: &ObjectStore) -> Result<Vec<T>, String> {
    let values = store.get_all(None, None).map_err(err)?.await.map_err(err)?;
    Ok(values
        .into_iter()
        .filter_map(|v| v.as_string())
        // A value this app version can't read is skipped rather than losing the rest.
        .filter_map(|json| serde_json::from_str(&json).ok())
        .collect())
}

fn put(store: &ObjectStore, key: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(err)?;
    // The request completes with the transaction, which is awaited below.
    store
        .put(&JsValue::from_str(&json), Some(&JsValue::from_str(key)))
        .map_err(err)?;
    Ok(())
}

impl Backend for IdbBackend {
    async fn load(&self) -> Result<Snapshot, String> {
        let tx = self
            .db
            .transaction(&STORES, TransactionMode::ReadOnly)
            .map_err(err)?;
        let mut records: Vec<LocalRecord> = all(&tx.object_store("records").map_err(err)?).await?;
        records.sort_by_key(|r| (r.entity, r.id));
        let mut outbox: Vec<OutboxEntry> = all(&tx.object_store("outbox").map_err(err)?).await?;
        outbox.sort_by_key(|e| e.seq);
        let meta: Option<Meta> = all(&tx.object_store("meta").map_err(err)?)
            .await?
            .into_iter()
            .next();
        Ok(Snapshot {
            meta: meta.unwrap_or_default(),
            records,
            outbox,
        })
    }

    async fn write(&self, ops: Vec<PersistOp>) -> Result<(), String> {
        let tx = self
            .db
            .transaction(&STORES, TransactionMode::ReadWrite)
            .map_err(err)?;
        let records = tx.object_store("records").map_err(err)?;
        let outbox = tx.object_store("outbox").map_err(err)?;
        let meta = tx.object_store("meta").map_err(err)?;
        for op in &ops {
            match op {
                PersistOp::PutRecord(r) => put(&records, &record_key(r.entity, r.id), r)?,
                PersistOp::RemoveRecord(e, id) => {
                    records
                        .delete(JsValue::from_str(&record_key(*e, *id)))
                        .map_err(err)?;
                }
                PersistOp::PutOutbox(e) => put(&outbox, &outbox_key(e.seq), e)?,
                PersistOp::RemoveOutbox(seq) => {
                    outbox
                        .delete(JsValue::from_str(&outbox_key(*seq)))
                        .map_err(err)?;
                }
                PersistOp::SetMeta(m) => put(&meta, "meta", m)?,
                PersistOp::Clear => {
                    for s in [&records, &outbox, &meta] {
                        s.clear().map_err(err)?;
                    }
                }
            }
        }
        let result = tx.commit().map_err(err)?.await.map_err(err)?;
        if result.is_committed() {
            Ok(())
        } else {
            Err("IndexedDB transaction aborted".into())
        }
    }
}
