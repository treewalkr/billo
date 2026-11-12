//! The sync engine (architecture §5): push the outbox, then pull the change feed.
//!
//! [`sync_once`] works on a shared [`LocalState`] and hands every resulting
//! [`PersistOp`] to `persist`; the state is only borrowed between awaits, so local
//! writes can happen while a sync is in flight. When to run it (start-up, after
//! writes, on reconnect, "Sync now", retries) is decided by the app (`store`).

use std::cell::RefCell;

use billo_api::sync::{
    DEFAULT_PULL_LIMIT, MAX_PUSH_MUTATIONS, PullResponse, PushRequest, PushResponse,
};

use crate::api::{ApiError, Client};
use crate::local::{LocalState, PersistOp, PushSummary};

/// The server side of sync.
#[allow(async_fn_in_trait)]
pub trait Remote {
    async fn push(&self, req: &PushRequest) -> Result<PushResponse, ApiError>;
    async fn pull(&self, since: i64, limit: u32) -> Result<PullResponse, ApiError>;
}

impl Remote for Client {
    async fn push(&self, req: &PushRequest) -> Result<PushResponse, ApiError> {
        self.sync_push(req).await
    }

    async fn pull(&self, since: i64, limit: u32) -> Result<PullResponse, ApiError> {
        self.sync_pull(since, limit).await
    }
}

/// What one sync did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub pushed: PushSummary,
    /// Records changed by the pull.
    pub pulled: u32,
}

/// Pushes the whole outbox (in batches, oldest first), then pulls until the feed is
/// exhausted. On error the outbox keeps whatever wasn't acknowledged.
pub async fn sync_once<R: Remote>(
    state: &RefCell<LocalState>,
    remote: &R,
    persist: &mut impl FnMut(Vec<PersistOp>),
) -> Result<SyncReport, ApiError> {
    let mut report = SyncReport::default();
    loop {
        let batch = state.borrow().batch(MAX_PUSH_MUTATIONS);
        if batch.is_empty() {
            break;
        }
        let res = remote
            .push(&PushRequest {
                mutations: batch.clone(),
            })
            .await?;
        let (ops, summary) = state.borrow_mut().apply_push(&res.results);
        persist(ops);
        let p = &mut report.pushed;
        p.acked += summary.acked;
        p.conflicts += summary.conflicts;
        p.rejected += summary.rejected;
        if summary.retry {
            p.retry = true;
            return Err(ApiError::Server(
                500,
                "the server could not apply a change".into(),
            ));
        }
        if summary.acked + summary.rejected == 0 {
            // Nothing matched; don't loop forever on a confused server.
            return Err(ApiError::Decode("push results did not match".into()));
        }
    }
    loop {
        let since = state.borrow().since();
        let page = remote.pull(since, DEFAULT_PULL_LIMIT).await?;
        let ops = state.borrow_mut().apply_pull(&page);
        report.pulled += u32::try_from(
            ops.iter()
                .filter(|o| matches!(o, PersistOp::PutRecord(_)))
                .count(),
        )
        .unwrap_or(u32::MAX);
        persist(ops);
        if !page.has_more || page.next_since <= since {
            break;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use billo_api::sync::{Change, Entity, EntityState, MutationOp, MutationResult};
    use billo_api::{ErrorCode, ErrorDetail};
    use serde_json::{Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;
    use crate::local::block_on;
    use crate::local::memory::MemoryBackend;

    #[derive(Clone)]
    struct Row {
        version: i64,
        deleted: bool,
        fields: Value,
    }

    /// A tiny server with the same rules as the real one: versions, last write wins
    /// with a conflict flag, idempotent mutation ids, a change feed.
    #[derive(Default)]
    struct FakeServer {
        rows: RefCell<HashMap<(Entity, Uuid), Row>>,
        log: RefCell<Vec<(i64, Entity, Uuid)>>,
        applied: RefCell<HashMap<Uuid, MutationResult>>,
        offline: Cell<bool>,
        /// Apply the next push but lose the response.
        drop_response: Cell<bool>,
        /// Reject puts whose `name` is this.
        reject_name: RefCell<Option<String>>,
        pushes: Cell<u32>,
    }

    impl FakeServer {
        fn state(&self, e: Entity, id: Uuid) -> Option<EntityState> {
            self.rows.borrow().get(&(e, id)).map(|r| EntityState {
                entity: e,
                id,
                version: r.version,
                deleted: r.deleted,
                record: (!r.deleted).then(|| {
                    let mut v = r.fields.clone();
                    v["id"] = json!(id);
                    v["version"] = json!(r.version);
                    v["created_at"] = json!("2026-11-12T10:00:00Z");
                    v["updated_at"] = json!("2026-11-12T10:00:00Z");
                    v
                }),
            })
        }

        /// A write by another device.
        fn write(&self, e: Entity, id: Uuid, fields: Option<Value>) {
            let mut rows = self.rows.borrow_mut();
            let prev = rows.get(&(e, id)).cloned();
            let version = prev.as_ref().map_or(1, |r| r.version + 1);
            let row = match fields {
                Some(fields) => Row {
                    version,
                    deleted: false,
                    fields,
                },
                None => Row {
                    version,
                    deleted: true,
                    fields: prev.map(|p| p.fields).unwrap_or(Value::Null),
                },
            };
            rows.insert((e, id), row);
            let mut log = self.log.borrow_mut();
            let seq = i64::try_from(log.len()).unwrap() + 1;
            log.push((seq, e, id));
        }

        fn name(&self, e: Entity, id: Uuid) -> Option<String> {
            let rows = self.rows.borrow();
            let r = rows.get(&(e, id))?;
            (!r.deleted).then(|| r.fields["name"].as_str().unwrap().to_owned())
        }
    }

    impl Remote for FakeServer {
        async fn push(&self, req: &PushRequest) -> Result<PushResponse, ApiError> {
            if self.offline.get() {
                return Err(ApiError::Network("offline".into()));
            }
            self.pushes.set(self.pushes.get() + 1);
            let mut results = Vec::new();
            for m in &req.mutations {
                if let Some(r) = self.applied.borrow().get(&m.mutation_id) {
                    results.push(r.clone());
                    continue;
                }
                let key = (m.entity, m.entity_id);
                let existing = self.rows.borrow().get(&key).cloned();
                let reject = |code, state| MutationResult {
                    mutation_id: m.mutation_id,
                    ok: false,
                    conflict: false,
                    version: None,
                    error: Some(ErrorDetail {
                        code,
                        message: "rejected".into(),
                    }),
                    state,
                };
                let conflict = existing
                    .as_ref()
                    .is_some_and(|r| m.base_version != Some(r.version));
                match m.op {
                    MutationOp::Put => {
                        let fields = m.payload.clone().unwrap();
                        if Some(fields["name"].as_str().unwrap_or(""))
                            == self.reject_name.borrow().as_deref()
                        {
                            results.push(reject(
                                ErrorCode::Validation,
                                self.state(m.entity, m.entity_id),
                            ));
                            continue;
                        }
                        self.write(m.entity, m.entity_id, Some(fields));
                    }
                    MutationOp::Delete => match existing {
                        None => {
                            results.push(reject(ErrorCode::NotFound, None));
                            continue;
                        }
                        Some(r) if r.deleted => {}
                        Some(_) => self.write(m.entity, m.entity_id, None),
                    },
                }
                let state = self.state(m.entity, m.entity_id).unwrap();
                let r = MutationResult {
                    mutation_id: m.mutation_id,
                    ok: true,
                    conflict: conflict && m.op == MutationOp::Put,
                    version: Some(state.version),
                    error: None,
                    state: Some(state),
                };
                self.applied.borrow_mut().insert(m.mutation_id, r.clone());
                results.push(r);
            }
            if self.drop_response.replace(false) {
                return Err(ApiError::Network("connection reset".into()));
            }
            Ok(PushResponse { results })
        }

        async fn pull(&self, since: i64, limit: u32) -> Result<PullResponse, ApiError> {
            if self.offline.get() {
                return Err(ApiError::Network("offline".into()));
            }
            let log = self.log.borrow();
            let page: Vec<_> = log
                .iter()
                .filter(|(s, _, _)| *s > since)
                .take(limit as usize)
                .collect();
            let has_more = log.iter().filter(|(s, _, _)| *s > since).count() > page.len();
            let next_since = page.last().map_or(since, |p| p.0);
            let mut latest: HashMap<(Entity, Uuid), i64> = HashMap::new();
            for (s, e, id) in &page {
                latest.insert((*e, *id), *s);
            }
            let mut changes: Vec<Change> = latest
                .into_iter()
                .map(|((e, id), seq)| Change {
                    seq,
                    state: self.state(e, id).unwrap(),
                })
                .collect();
            changes.sort_by_key(|c| c.seq);
            Ok(PullResponse {
                changes,
                next_since,
                has_more,
            })
        }
    }

    /// A device: local state plus its durable copy.
    struct Device {
        state: RefCell<LocalState>,
        disk: MemoryBackend,
    }

    impl Device {
        fn new() -> Self {
            let d = Self {
                state: RefCell::new(LocalState::default()),
                disk: MemoryBackend::default(),
            };
            let ops = d.state.borrow_mut().set_user(Uuid::nil());
            d.disk.apply(&ops);
            d
        }

        fn put(&self, id: Uuid, name: &str) {
            let (_, ops) = self.state.borrow_mut().put(
                Entity::Account,
                id,
                json!({ "kind": "cash", "name": name, "currency": "THB" }),
                Uuid::now_v7(),
                OffsetDateTime::UNIX_EPOCH,
            );
            self.disk.apply(&ops);
        }

        fn delete(&self, id: Uuid) {
            let ops = self
                .state
                .borrow_mut()
                .delete(Entity::Account, id, Uuid::now_v7())
                .unwrap();
            self.disk.apply(&ops);
        }

        fn sync(&self, server: &FakeServer) -> Result<SyncReport, ApiError> {
            block_on(sync_once(&self.state, server, &mut |ops| {
                self.disk.apply(&ops)
            }))
        }

        fn name(&self, id: Uuid) -> Option<String> {
            self.state
                .borrow()
                .get(Entity::Account, id)
                .map(|r| r["name"].as_str().unwrap().to_owned())
        }

        fn pending(&self) -> usize {
            self.state.borrow().pending()
        }

        /// Restarting the app: memory is rebuilt from disk.
        fn restart(&self) {
            let snap = block_on(crate::local::Backend::load(&self.disk)).unwrap();
            *self.state.borrow_mut() = LocalState::from_snapshot(snap);
        }
    }

    #[test]
    fn offline_write_shows_at_once_and_is_pushed_later() {
        let server = FakeServer::default();
        let dev = Device::new();
        server.offline.set(true);
        let id = Uuid::now_v7();
        dev.put(id, "Wallet");
        assert_eq!(dev.name(id).as_deref(), Some("Wallet"));
        assert_eq!(dev.pending(), 1);
        assert!(matches!(dev.sync(&server), Err(ApiError::Network(_))));
        assert_eq!(dev.pending(), 1);

        // Survives a restart while offline.
        dev.restart();
        assert_eq!(dev.name(id).as_deref(), Some("Wallet"));
        assert_eq!(dev.pending(), 1);

        server.offline.set(false);
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pushed.acked, 1);
        assert_eq!(dev.pending(), 0);
        assert_eq!(server.name(Entity::Account, id).as_deref(), Some("Wallet"));
        // The local copy now carries the server's version.
        assert_eq!(
            dev.state
                .borrow()
                .local_record(Entity::Account, id)
                .unwrap()
                .version,
            1
        );
        assert_eq!(dev.state.borrow().since(), 1);
        dev.restart();
        assert_eq!(dev.pending(), 0);
    }

    #[test]
    fn lost_ack_is_replayed_safely() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        dev.put(id, "Wallet");
        server.drop_response.set(true);
        assert!(dev.sync(&server).is_err());
        // Applied on the server, still queued here.
        assert_eq!(server.log.borrow().len(), 1);
        assert_eq!(dev.pending(), 1);
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pushed.acked, 1);
        assert_eq!(report.pushed.conflicts, 0);
        assert_eq!(dev.pending(), 0);
        // Not applied twice.
        assert_eq!(server.log.borrow().len(), 1);
    }

    #[test]
    fn edits_of_one_record_are_chained_without_false_conflicts() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        let other = Uuid::now_v7();
        dev.put(id, "A");
        dev.put(other, "X");
        dev.put(id, "B");
        dev.put(id, "C");
        // One mutation per record per batch.
        assert_eq!(dev.state.borrow().batch(200).len(), 2);
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pushed.acked, 4);
        assert_eq!(report.pushed.conflicts, 0);
        assert_eq!(server.pushes.get(), 3);
        assert_eq!(server.name(Entity::Account, id).as_deref(), Some("C"));
        assert_eq!(dev.name(id).as_deref(), Some("C"));
    }

    #[test]
    fn remote_update_applies_when_nothing_is_pending() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        dev.put(id, "Wallet");
        dev.sync(&server).unwrap();
        server.write(Entity::Account, id, Some(json!({ "name": "Purse" })));
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pulled, 1);
        assert_eq!(dev.name(id).as_deref(), Some("Purse"));
        assert_eq!(
            dev.state
                .borrow()
                .local_record(Entity::Account, id)
                .unwrap()
                .version,
            2
        );
        assert_eq!(dev.state.borrow().since(), 2);
    }

    #[test]
    fn pending_local_change_wins_over_remote_update_and_is_flagged() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        dev.put(id, "Wallet");
        dev.sync(&server).unwrap();
        // Another device changes it while this one edits offline.
        server.write(Entity::Account, id, Some(json!({ "name": "Theirs" })));
        dev.put(id, "Mine");
        // The pull while the edit is pending doesn't overwrite it.
        let page = block_on(server.pull(dev.state.borrow().since(), 100)).unwrap();
        let ops = dev.state.borrow_mut().apply_pull(&page);
        assert!(ops.iter().all(|o| matches!(o, PersistOp::SetMeta(_))));
        assert_eq!(dev.name(id).as_deref(), Some("Mine"));
        // Pushed over the newer server version: applied, with the conflict reported.
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pushed.conflicts, 1);
        assert_eq!(server.name(Entity::Account, id).as_deref(), Some("Mine"));
        assert_eq!(dev.name(id).as_deref(), Some("Mine"));
        assert_eq!(
            dev.state
                .borrow()
                .local_record(Entity::Account, id)
                .unwrap()
                .version,
            3
        );
    }

    #[test]
    fn tombstones_remove_records_and_stop_older_states() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        server.write(Entity::Account, id, Some(json!({ "name": "Old" })));
        dev.sync(&server).unwrap();
        assert_eq!(dev.name(id).as_deref(), Some("Old"));
        let stale = server.state(Entity::Account, id).unwrap();
        server.write(Entity::Account, id, None);
        dev.sync(&server).unwrap();
        assert_eq!(dev.name(id), None);
        let tomb = dev
            .state
            .borrow()
            .local_record(Entity::Account, id)
            .cloned()
            .unwrap();
        assert!(tomb.deleted);
        assert_eq!(tomb.version, 2);
        // A late, older state doesn't bring it back.
        assert_eq!(dev.state.borrow_mut().apply_remote(&stale), None);
        assert_eq!(dev.name(id), None);
        assert_eq!(dev.state.borrow().records(Entity::Account).count(), 0);
    }

    #[test]
    fn local_delete_is_pushed() {
        let server = FakeServer::default();
        let dev = Device::new();
        let id = Uuid::now_v7();
        dev.put(id, "Wallet");
        dev.sync(&server).unwrap();
        dev.delete(id);
        assert_eq!(dev.name(id), None);
        assert!(
            dev.state
                .borrow_mut()
                .delete(Entity::Account, id, Uuid::now_v7())
                .is_none()
        );
        dev.sync(&server).unwrap();
        assert_eq!(server.name(Entity::Account, id), None);
        assert_eq!(dev.pending(), 0);
    }

    #[test]
    fn rejected_change_is_dropped_and_server_state_restored() {
        let server = FakeServer::default();
        let dev = Device::new();
        let kept = Uuid::now_v7();
        let fresh = Uuid::now_v7();
        dev.put(kept, "Wallet");
        dev.sync(&server).unwrap();
        *server.reject_name.borrow_mut() = Some("bad".into());
        dev.put(kept, "bad");
        dev.put(fresh, "bad");
        let after = Uuid::now_v7();
        dev.put(after, "Fine");
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pushed.rejected, 2);
        assert_eq!(report.pushed.acked, 1);
        assert_eq!(dev.pending(), 0);
        // The edit is undone, the never-accepted record is gone, the rest went through.
        assert_eq!(dev.name(kept).as_deref(), Some("Wallet"));
        assert_eq!(dev.name(fresh), None);
        assert_eq!(dev.name(after).as_deref(), Some("Fine"));
        dev.restart();
        assert_eq!(dev.name(fresh), None);
    }

    #[test]
    fn cursor_advances_over_pages_and_survives_restart() {
        let server = FakeServer::default();
        let dev = Device::new();
        let ids: Vec<Uuid> = (0..1200).map(|_| Uuid::now_v7()).collect();
        for (i, id) in ids.iter().enumerate() {
            server.write(
                Entity::Account,
                *id,
                Some(json!({ "name": format!("n{i}") })),
            );
        }
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pulled, 1200);
        assert_eq!(dev.state.borrow().since(), 1200);
        dev.restart();
        assert_eq!(dev.state.borrow().since(), 1200);
        assert_eq!(dev.state.borrow().records(Entity::Account).count(), 1200);
        // Nothing new: the cursor stays and nothing changes.
        let report = dev.sync(&server).unwrap();
        assert_eq!(report.pulled, 0);
        assert_eq!(dev.state.borrow().since(), 1200);
        server.write(Entity::Account, ids[5], Some(json!({ "name": "renamed" })));
        dev.sync(&server).unwrap();
        assert_eq!(dev.state.borrow().since(), 1201);
        assert_eq!(dev.name(ids[5]).as_deref(), Some("renamed"));
    }

    #[test]
    fn server_failure_keeps_the_rest_queued_in_order() {
        let dev = Device::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        dev.put(a, "A");
        dev.put(b, "B");
        let first = dev.state.borrow().batch(10)[0].mutation_id;
        let res = [MutationResult {
            mutation_id: first,
            ok: false,
            conflict: false,
            version: None,
            error: Some(ErrorDetail {
                code: ErrorCode::Internal,
                message: "db down".into(),
            }),
            state: None,
        }];
        let (ops, summary) = dev.state.borrow_mut().apply_push(&res);
        assert!(summary.retry);
        assert!(ops.is_empty());
        assert_eq!(dev.pending(), 2);
    }

    #[test]
    fn another_user_clears_the_store() {
        let dev = Device::new();
        dev.put(Uuid::now_v7(), "Wallet");
        let ops = dev.state.borrow_mut().set_user(Uuid::nil());
        assert!(ops.is_empty());
        let ops = dev.state.borrow_mut().set_user(Uuid::now_v7());
        dev.disk.apply(&ops);
        assert_eq!(dev.pending(), 0);
        assert_eq!(dev.state.borrow().records(Entity::Account).count(), 0);
        dev.restart();
        assert_eq!(dev.pending(), 0);
        assert_eq!(dev.state.borrow().since(), 0);
    }
}
