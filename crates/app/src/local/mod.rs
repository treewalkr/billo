//! The local store (architecture §5): every record the user has, the outbox of changes
//! not yet on the server, and the change-feed cursor.
//!
//! [`LocalState`] is the in-memory model and all merge rules; it is plain Rust so the
//! rules can be tested without a browser. Each change returns the [`PersistOp`]s that
//! make it durable, which a [`Backend`] writes: IndexedDB on the web, SQLite on
//! iOS / Android. At start-up the backend's [`Snapshot`] is loaded back into memory.

use std::collections::{BTreeMap, HashMap};

use billo_api::sync::{Entity, EntityState, Mutation, MutationOp, MutationResult, PullResponse};
use billo_api::{ErrorCode, ErrorDetail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg(target_arch = "wasm32")]
mod idb;
#[cfg(test)]
pub mod memory;
#[cfg(not(target_arch = "wasm32"))]
mod sqlite;

#[cfg(target_arch = "wasm32")]
pub use idb::IdbBackend as PlatformBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use sqlite::SqliteBackend as PlatformBackend;

/// One record as the app knows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalRecord {
    pub entity: Entity,
    pub id: Uuid,
    /// The last server version this device has seen; 0 if the server hasn't
    /// acknowledged the record yet.
    pub version: i64,
    /// A tombstone. Kept so an older state arriving later can't bring it back.
    pub deleted: bool,
    /// The record as `Record<D>` JSON; `None` for tombstones.
    pub record: Option<Value>,
}

/// A queued mutation and its place in the outbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub seq: u64,
    pub mutation: Mutation,
}

/// Small values kept next to the records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// Whose data this is; another user signing in clears everything.
    pub user_id: Option<Uuid>,
    /// Change-feed cursor: the last `seq` applied.
    pub since: i64,
    /// Next outbox `seq`.
    pub next_seq: u64,
}

/// A write for the backend. Applied in order.
#[derive(Debug, Clone, PartialEq)]
pub enum PersistOp {
    PutRecord(LocalRecord),
    RemoveRecord(Entity, Uuid),
    PutOutbox(OutboxEntry),
    RemoveOutbox(u64),
    SetMeta(Meta),
    /// Remove everything.
    Clear,
}

/// Everything a backend holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub meta: Meta,
    pub records: Vec<LocalRecord>,
    pub outbox: Vec<OutboxEntry>,
}

/// Durable storage for the local store.
#[allow(async_fn_in_trait)]
pub trait Backend {
    async fn load(&self) -> Result<Snapshot, String>;
    /// Writes `ops` atomically, in order.
    async fn write(&self, ops: Vec<PersistOp>) -> Result<(), String>;
}

/// What a push changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushSummary {
    /// Mutations the server applied (now or before).
    pub acked: u32,
    /// Applied, but over a newer server version (last write wins).
    pub conflicts: u32,
    /// Rejected by the server and dropped; the server's state was restored locally.
    pub rejected: u32,
    /// The server failed on a mutation; it and the rest stay queued.
    pub retry: bool,
    /// Why the last rejected mutation was rejected (the server's code and message).
    pub rejection: Option<ErrorDetail>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalState {
    meta: Meta,
    records: HashMap<(Entity, Uuid), LocalRecord>,
    outbox: BTreeMap<u64, Mutation>,
}

impl LocalState {
    pub fn from_snapshot(s: Snapshot) -> Self {
        Self {
            meta: s.meta,
            records: s
                .records
                .into_iter()
                .map(|r| ((r.entity, r.id), r))
                .collect(),
            outbox: s.outbox.into_iter().map(|e| (e.seq, e.mutation)).collect(),
        }
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> Snapshot {
        let mut records: Vec<_> = self.records.values().cloned().collect();
        records.sort_by_key(|r| (r.entity, r.id));
        Snapshot {
            meta: self.meta.clone(),
            records,
            outbox: self
                .outbox
                .iter()
                .map(|(&seq, m)| OutboxEntry {
                    seq,
                    mutation: m.clone(),
                })
                .collect(),
        }
    }

    pub fn user_id(&self) -> Option<Uuid> {
        self.meta.user_id
    }

    pub fn since(&self) -> i64 {
        self.meta.since
    }

    /// Changes waiting to be pushed.
    pub fn pending(&self) -> usize {
        self.outbox.len()
    }

    fn has_pending(&self, entity: Entity, id: Uuid) -> bool {
        self.outbox
            .values()
            .any(|m| m.entity == entity && m.entity_id == id)
    }

    /// Makes the store belong to `user_id`; another user's data (or no user's) is
    /// cleared first.
    pub fn set_user(&mut self, user_id: Uuid) -> Vec<PersistOp> {
        if self.meta.user_id == Some(user_id) {
            return Vec::new();
        }
        let mut ops = self.clear();
        self.meta.user_id = Some(user_id);
        ops.push(PersistOp::SetMeta(self.meta.clone()));
        ops
    }

    /// Forgets everything (sign-out).
    pub fn clear(&mut self) -> Vec<PersistOp> {
        *self = Self::default();
        vec![PersistOp::Clear]
    }

    /// Live (not deleted) records of `entity`, in no particular order.
    pub fn records(&self, entity: Entity) -> impl Iterator<Item = &Value> {
        self.records
            .values()
            .filter(move |r| r.entity == entity && !r.deleted)
            .filter_map(|r| r.record.as_ref())
    }

    /// One live record.
    pub fn get(&self, entity: Entity, id: Uuid) -> Option<&Value> {
        self.records
            .get(&(entity, id))
            .filter(|r| !r.deleted)
            .and_then(|r| r.record.as_ref())
    }

    #[cfg(test)]
    pub fn local_record(&self, entity: Entity, id: Uuid) -> Option<&LocalRecord> {
        self.records.get(&(entity, id))
    }

    fn enqueue(&mut self, mutation: Mutation, ops: &mut Vec<PersistOp>) {
        let seq = self.meta.next_seq;
        self.meta.next_seq += 1;
        self.outbox.insert(seq, mutation.clone());
        ops.push(PersistOp::PutOutbox(OutboxEntry { seq, mutation }));
        ops.push(PersistOp::SetMeta(self.meta.clone()));
    }

    fn base_version(&self, entity: Entity, id: Uuid) -> Option<i64> {
        self.records
            .get(&(entity, id))
            .map(|r| r.version)
            .filter(|&v| v > 0)
    }

    /// Creates or updates a record locally and queues the change. `fields` is the
    /// resource's data as a JSON object. Returns the record as stored locally.
    pub fn put(
        &mut self,
        entity: Entity,
        id: Uuid,
        fields: Value,
        mutation_id: Uuid,
        now: OffsetDateTime,
    ) -> (Value, Vec<PersistOp>) {
        let existing = self.records.get(&(entity, id));
        let version = existing.map_or(0, |r| r.version);
        let created_at = existing
            .and_then(|r| r.record.as_ref())
            .and_then(|r| r.get("created_at").cloned())
            .unwrap_or_else(|| rfc3339(now));
        let mut record = fields.clone();
        if let Value::Object(map) = &mut record {
            map.insert("id".into(), Value::String(id.to_string()));
            map.insert("version".into(), Value::from(version));
            map.insert("created_at".into(), created_at);
            map.insert("updated_at".into(), rfc3339(now));
        }
        let base_version = self.base_version(entity, id);
        let local = LocalRecord {
            entity,
            id,
            version,
            deleted: false,
            record: Some(record.clone()),
        };
        self.records.insert((entity, id), local.clone());
        let mut ops = vec![PersistOp::PutRecord(local)];
        self.enqueue(
            Mutation {
                mutation_id,
                entity,
                entity_id: id,
                base_version,
                op: MutationOp::Put,
                payload: Some(fields),
            },
            &mut ops,
        );
        (record, ops)
    }

    /// Deletes a record locally and queues the change. `None` if there is no live
    /// record with this id.
    pub fn delete(
        &mut self,
        entity: Entity,
        id: Uuid,
        mutation_id: Uuid,
    ) -> Option<Vec<PersistOp>> {
        self.get(entity, id)?;
        let base_version = self.base_version(entity, id);
        let version = self.records.get(&(entity, id)).map_or(0, |r| r.version);
        let local = LocalRecord {
            entity,
            id,
            version,
            deleted: true,
            record: None,
        };
        self.records.insert((entity, id), local.clone());
        let mut ops = vec![PersistOp::PutRecord(local)];
        self.enqueue(
            Mutation {
                mutation_id,
                entity,
                entity_id: id,
                base_version,
                op: MutationOp::Delete,
                payload: None,
            },
            &mut ops,
        );
        Some(ops)
    }

    /// The next mutations to push, oldest first: at most `max`, and at most one per
    /// record, so each one's `base_version` can be set from the previous one's result.
    pub fn batch(&self, max: usize) -> Vec<Mutation> {
        let mut seen = std::collections::HashSet::new();
        self.outbox
            .values()
            .take_while(|m| seen.insert((m.entity, m.entity_id)))
            .take(max)
            .cloned()
            .collect()
    }

    /// Applies the server's results for pushed mutations.
    pub fn apply_push(&mut self, results: &[MutationResult]) -> (Vec<PersistOp>, PushSummary) {
        let mut ops = Vec::new();
        let mut summary = PushSummary::default();
        for r in results {
            let Some((&seq, m)) = self
                .outbox
                .iter()
                .find(|(_, m)| m.mutation_id == r.mutation_id)
            else {
                continue;
            };
            let (entity, id) = (m.entity, m.entity_id);
            if !r.ok
                && r.error
                    .as_ref()
                    .is_none_or(|e| e.code == ErrorCode::Internal)
            {
                // Worth retrying; later mutations may depend on it, so stop here.
                summary.retry = true;
                break;
            }
            self.outbox.remove(&seq);
            ops.push(PersistOp::RemoveOutbox(seq));
            if r.ok {
                summary.acked += 1;
                if r.conflict {
                    summary.conflicts += 1;
                }
                // Later edits of the same record build on this version.
                if let Some(v) = r.version {
                    for (&s, later) in self.outbox.iter_mut() {
                        if later.entity == entity && later.entity_id == id {
                            later.base_version = Some(v);
                            ops.push(PersistOp::PutOutbox(OutboxEntry {
                                seq: s,
                                mutation: later.clone(),
                            }));
                        }
                    }
                }
            } else {
                summary.rejected += 1;
                summary.rejection.clone_from(&r.error);
            }
            if self.has_pending(entity, id) {
                continue;
            }
            match &r.state {
                Some(state) => ops.extend(self.apply_remote(state)),
                // The server has no such record for this user: drop the local copy.
                None if !r.ok && self.records.remove(&(entity, id)).is_some() => {
                    ops.push(PersistOp::RemoveRecord(entity, id));
                }
                None => {}
            }
        }
        (ops, summary)
    }

    /// Takes the server's state of a record, unless a local change is still waiting
    /// to be pushed (it wins and will be pushed) or the state is older than ours.
    pub fn apply_remote(&mut self, state: &EntityState) -> Option<PersistOp> {
        if self.has_pending(state.entity, state.id) {
            return None;
        }
        if let Some(local) = self.records.get(&(state.entity, state.id))
            && local.version > state.version
        {
            return None;
        }
        let local = LocalRecord {
            entity: state.entity,
            id: state.id,
            version: state.version,
            deleted: state.deleted,
            record: if state.deleted {
                None
            } else {
                state.record.clone()
            },
        };
        if self.records.get(&(state.entity, state.id)) == Some(&local) {
            return None;
        }
        self.records.insert((state.entity, state.id), local.clone());
        Some(PersistOp::PutRecord(local))
    }

    /// Applies a page of the change feed and advances the cursor.
    pub fn apply_pull(&mut self, page: &PullResponse) -> Vec<PersistOp> {
        let mut ops: Vec<PersistOp> = page
            .changes
            .iter()
            .filter_map(|c| self.apply_remote(&c.state))
            .collect();
        if page.next_since > self.meta.since {
            self.meta.since = page.next_since;
            ops.push(PersistOp::SetMeta(self.meta.clone()));
        }
        ops
    }
}

fn rfc3339(t: OffsetDateTime) -> Value {
    t.format(&time::format_description::well_known::Rfc3339)
        .map_or(Value::Null, Value::String)
}

/// Applies `ops` to a snapshot; what every backend does, in memory.
#[cfg(test)]
pub fn apply_ops(snapshot: &mut Snapshot, ops: &[PersistOp]) {
    for op in ops {
        match op {
            PersistOp::PutRecord(r) => {
                snapshot
                    .records
                    .retain(|x| (x.entity, x.id) != (r.entity, r.id));
                snapshot.records.push(r.clone());
            }
            PersistOp::RemoveRecord(e, id) => {
                snapshot.records.retain(|x| (x.entity, x.id) != (*e, *id));
            }
            PersistOp::PutOutbox(e) => {
                snapshot.outbox.retain(|x| x.seq != e.seq);
                snapshot.outbox.push(e.clone());
                snapshot.outbox.sort_by_key(|x| x.seq);
            }
            PersistOp::RemoveOutbox(seq) => snapshot.outbox.retain(|x| x.seq != *seq),
            PersistOp::SetMeta(m) => snapshot.meta = m.clone(),
            PersistOp::Clear => *snapshot = Snapshot::default(),
        }
    }
    snapshot.records.sort_by_key(|r| (r.entity, r.id));
}

/// Storage keys shared by the backends.
pub(crate) fn record_key(entity: Entity, id: Uuid) -> String {
    format!("{}:{id}", entity.as_str())
}

/// Zero-padded so keys sort in outbox order.
pub(crate) fn outbox_key(seq: u64) -> String {
    format!("{seq:020}")
}

/// Runs a future that never waits on anything external (in-memory backends and fakes).
#[cfg(test)]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}
