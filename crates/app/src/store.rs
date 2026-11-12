//! The app's data: every screen reads from and writes to the local store (architecture
//! §5), never straight to the server.
//!
//! [`Store`] keeps a [`LocalState`] in memory, loaded at start-up from the platform
//! backend (IndexedDB on the web, SQLite on iOS / Android), and writes every change back
//! to it in order. Writes are applied locally at once and queued in the outbox; the sync
//! engine (`ui::sync`) pushes them and pulls the server's changes.
//!
//! The typed methods mirror [`crate::api::Client`], so screens use [`Store::call`] the
//! way they used `Auth::call`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use billo_api::sync::Entity;
use billo_api::{
    AccountData, BudgetData, BudgetQuery, CategoryData, InstallmentPlanData, Record,
    TransactionData, TransactionQuery,
};
use dioxus::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::api::{self, ApiError, Client};
use crate::local::{Backend, LocalState, PersistOp, PlatformBackend};
use crate::sync::{SyncReport, sync_once};

/// What the sync indicator shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Synced,
    Syncing,
    /// Changes waiting to be pushed.
    Waiting(usize),
    /// No connection; changes waiting.
    Offline(usize),
    /// The last sync failed for another reason; changes waiting.
    Error(usize),
}

/// The sync engine's state, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncInfo {
    pub syncing: bool,
    /// The device reports a connection (web: `navigator.onLine`).
    pub online: bool,
    /// The last sync couldn't reach the server.
    pub unreachable: bool,
    /// The last sync failed otherwise.
    pub error: Option<String>,
    /// Changes the server applied over someone else's newer version, not yet noticed.
    pub conflicts: u32,
    /// Changes the server rejected (and that were undone), not yet noticed.
    pub rejected: u32,
    /// The local database couldn't be opened; data lives in memory only.
    pub store_failed: bool,
}

impl Default for SyncInfo {
    fn default() -> Self {
        Self {
            syncing: false,
            online: true,
            unreachable: false,
            error: None,
            conflicts: 0,
            rejected: 0,
            store_failed: false,
        }
    }
}

impl SyncInfo {
    pub fn status(&self, pending: usize) -> SyncStatus {
        if self.syncing {
            SyncStatus::Syncing
        } else if !self.online || self.unreachable {
            SyncStatus::Offline(pending)
        } else if self.error.is_some() {
            SyncStatus::Error(pending)
        } else if pending > 0 {
            SyncStatus::Waiting(pending)
        } else {
            SyncStatus::Synced
        }
    }
}

/// What sign-out does once it has tried to push the waiting changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignOutCheck {
    /// Nothing is waiting: sign out and clear the local data.
    Clear,
    /// Changes are still waiting: ask before discarding them.
    Confirm {
        waiting: usize,
        /// No connection (otherwise the sync failed for another reason).
        offline: bool,
    },
}

/// Decides sign-out after the push attempt: local data is only cleared when nothing is
/// left to send.
pub fn sign_out_check(info: &SyncInfo, pending: usize) -> SignOutCheck {
    if pending == 0 {
        SignOutCheck::Clear
    } else {
        SignOutCheck::Confirm {
            waiting: pending,
            offline: !info.online || info.unreachable,
        }
    }
}

/// Why the sync engine should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMsg {
    /// Run now (start-up, sign-in, "Sync now", back online).
    Now,
    /// A local write; run after a short quiet period.
    Changed,
    /// The device's connection came or went (web `online` / `offline` events).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Online(bool),
}

/// Writes ops to the backend in order, one batch at a time.
#[derive(Default)]
struct Writer {
    backend: RefCell<Option<Rc<PlatformBackend>>>,
    queue: RefCell<Vec<PersistOp>>,
    flushing: Cell<bool>,
}

impl Writer {
    fn push(self: &Rc<Self>, ops: Vec<PersistOp>) {
        if ops.is_empty() {
            return;
        }
        self.queue.borrow_mut().extend(ops);
        self.flush();
    }

    fn flush(self: &Rc<Self>) {
        let Some(backend) = self.backend.borrow().clone() else {
            return;
        };
        if self.flushing.replace(true) {
            return;
        }
        let this = self.clone();
        // Not tied to a screen: a write must finish even if the user navigates away.
        dioxus::dioxus_core::spawn_forever(async move {
            loop {
                let ops = std::mem::take(&mut *this.queue.borrow_mut());
                if ops.is_empty() {
                    break;
                }
                if let Err(e) = backend.write(ops).await {
                    tracing_warn(&format!("local store write failed: {e}"));
                }
            }
            this.flushing.set(false);
        });
    }
}

fn tracing_warn(msg: &str) {
    dioxus::logger::tracing::warn!("{msg}");
}

/// The local store, shared through context. `Copy`, so it can move into closures.
#[derive(Clone, Copy)]
pub struct Store {
    state: CopyValue<Rc<RefCell<LocalState>>>,
    writer: CopyValue<Rc<Writer>>,
    /// Bumped on every change, so readers re-run.
    rev: Signal<u64>,
    ready: Signal<bool>,
    info: Signal<SyncInfo>,
    engine: CopyValue<Option<Coroutine<SyncMsg>>>,
}

pub fn use_store() -> Store {
    use_context::<Store>()
}

impl Store {
    /// Creates the store (empty until [`Store::open`] finishes). Call in the root
    /// component.
    pub fn create() -> Self {
        Self {
            state: CopyValue::new(Rc::default()),
            writer: CopyValue::new(Rc::default()),
            rev: Signal::new(0),
            ready: Signal::new(false),
            info: Signal::new(SyncInfo::default()),
            engine: CopyValue::new(None),
        }
    }

    /// Loads the local database into memory.
    pub async fn open(mut self) {
        match PlatformBackend::open().await {
            Ok(backend) => match backend.load().await {
                Ok(snapshot) => {
                    *self.shared().borrow_mut() = LocalState::from_snapshot(snapshot);
                    let writer = self.writer.read().clone();
                    *writer.backend.borrow_mut() = Some(Rc::new(backend));
                    // Anything written before the database was open.
                    writer.flush();
                }
                Err(e) => self.store_failed(&e),
            },
            Err(e) => self.store_failed(&e),
        }
        self.ready.set(true);
        self.bump();
    }

    fn store_failed(mut self, e: &str) {
        tracing_warn(&format!("local store unavailable: {e}"));
        self.info.write().store_failed = true;
    }

    pub fn set_engine(self, engine: Coroutine<SyncMsg>) {
        let mut slot = self.engine;
        *slot.write() = Some(engine);
    }

    fn shared(&self) -> Rc<RefCell<LocalState>> {
        self.state.read().clone()
    }

    fn persist(&self, ops: Vec<PersistOp>) {
        self.writer.read().push(ops);
    }

    fn bump(mut self) {
        *self.rev.write() += 1;
    }

    /// Subscribes the caller to changes.
    fn track(&self) {
        let _ = self.rev.read();
    }

    pub fn ready(&self) -> bool {
        *self.ready.read()
    }

    pub fn info(&self) -> SyncInfo {
        self.info.read().clone()
    }

    /// Changes waiting to be pushed.
    pub fn pending(&self) -> usize {
        self.track();
        self.shared().borrow().pending()
    }

    /// [`Store::pending`] without subscribing (for the engine).
    pub fn pending_now(&self) -> usize {
        self.shared().borrow().pending()
    }

    pub fn status(&self) -> SyncStatus {
        self.info.read().status(self.pending())
    }

    pub fn set_online(mut self, online: bool) {
        let mut info = self.info.write();
        info.online = online;
        if online {
            info.unreachable = false;
        }
    }

    pub fn dismiss_notices(mut self) {
        let mut info = self.info.write();
        info.conflicts = 0;
        info.rejected = 0;
    }

    pub fn send(&self, msg: SyncMsg) {
        if let Some(engine) = *self.engine.read() {
            engine.send(msg);
        }
    }

    /// Makes the store belong to `user_id` (clearing another user's data).
    pub fn set_user(self, user_id: Uuid) {
        let ops = self.shared().borrow_mut().set_user(user_id);
        if !ops.is_empty() {
            self.persist(ops);
            self.reset_info();
            self.bump();
        }
    }

    /// Forgets all local data (sign-out).
    pub fn clear(self) {
        let state = self.shared();
        if state.borrow().user_id().is_none() && state.borrow().pending() == 0 {
            return;
        }
        let ops = state.borrow_mut().clear();
        self.persist(ops);
        self.reset_info();
        self.bump();
    }

    fn reset_info(mut self) {
        let mut info = self.info.write();
        let keep = (info.online, info.store_failed);
        *info = SyncInfo::default();
        (info.online, info.store_failed) = keep;
    }

    /// One push + pull with `client`. Updates the sync status; the caller handles
    /// `Unauthorized`.
    pub async fn sync(mut self, client: Client) -> Result<SyncReport, ApiError> {
        self.info.write().syncing = true;
        let state = self.shared();
        let mut persist = |ops: Vec<PersistOp>| {
            if !ops.is_empty() {
                self.persist(ops);
                self.bump();
            }
        };
        let result = sync_once(&state, &client, &mut persist).await;
        let mut info = self.info.write();
        info.syncing = false;
        match &result {
            Ok(report) => {
                info.unreachable = false;
                info.error = None;
                info.conflicts += report.pushed.conflicts;
                info.rejected += report.pushed.rejected;
            }
            Err(ApiError::Network(_)) => info.unreachable = true,
            Err(ApiError::Unauthorized) => {}
            Err(e) => {
                info.unreachable = false;
                info.error = Some(e.to_string());
            }
        }
        drop(info);
        self.bump();
        result
    }

    /// Runs `f` against the store, like `Auth::call` does against the server.
    pub async fn call<T, F, Fut>(self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(Self) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        f(self).await
    }

    fn list<D: DeserializeOwned>(&self, entity: Entity) -> Result<Vec<Record<D>>, ApiError> {
        self.track();
        let state = self.shared();
        let state = state.borrow();
        state
            .records(entity)
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| ApiError::Decode(e.to_string())))
            .collect()
    }

    fn get<D: DeserializeOwned>(&self, entity: Entity, id: Uuid) -> Result<Record<D>, ApiError> {
        self.track();
        let state = self.shared();
        let state = state.borrow();
        let v = state.get(entity, id).ok_or(ApiError::NotFound)?;
        serde_json::from_value(v.clone()).map_err(|e| ApiError::Decode(e.to_string()))
    }

    fn put<D: Serialize + DeserializeOwned>(
        self,
        entity: Entity,
        id: Uuid,
        data: D,
    ) -> Result<Record<D>, ApiError> {
        let fields = serde_json::to_value(&data).map_err(|e| ApiError::Decode(e.to_string()))?;
        let (record, ops) = self.shared().borrow_mut().put(
            entity,
            id,
            fields,
            api::new_id(),
            time::OffsetDateTime::now_utc(),
        );
        self.wrote(ops);
        serde_json::from_value(record).map_err(|e| ApiError::Decode(e.to_string()))
    }

    fn delete(self, entity: Entity, id: Uuid) -> Result<(), ApiError> {
        let ops = self
            .shared()
            .borrow_mut()
            .delete(entity, id, api::new_id())
            .ok_or(ApiError::NotFound)?;
        self.wrote(ops);
        Ok(())
    }

    fn wrote(self, ops: Vec<PersistOp>) {
        self.persist(ops);
        self.bump();
        self.send(SyncMsg::Changed);
    }

    pub async fn accounts(self) -> Result<Vec<Record<AccountData>>, ApiError> {
        let mut list: Vec<Record<AccountData>> = self.list(Entity::Account)?;
        list.sort_by_key(|r| (r.created_at, r.id));
        Ok(list)
    }

    pub async fn account(self, id: Uuid) -> Result<Record<AccountData>, ApiError> {
        self.get(Entity::Account, id)
    }

    pub async fn put_account(
        self,
        id: Uuid,
        data: AccountData,
    ) -> Result<Record<AccountData>, ApiError> {
        self.put(Entity::Account, id, data)
    }

    pub async fn delete_account(self, id: Uuid) -> Result<(), ApiError> {
        self.delete(Entity::Account, id)
    }

    pub async fn categories(self) -> Result<Vec<Record<CategoryData>>, ApiError> {
        let mut list: Vec<Record<CategoryData>> = self.list(Entity::Category)?;
        list.sort_by_key(|r| (r.created_at, r.id));
        Ok(list)
    }

    pub async fn put_category(
        self,
        id: Uuid,
        data: CategoryData,
    ) -> Result<Record<CategoryData>, ApiError> {
        self.put(Entity::Category, id, data)
    }

    pub async fn delete_category(self, id: Uuid) -> Result<(), ApiError> {
        self.delete(Entity::Category, id)
    }

    /// Newest first (by `occurred_on`), like the server's list.
    pub async fn transactions(
        self,
        q: &TransactionQuery,
    ) -> Result<Vec<Record<TransactionData>>, ApiError> {
        let mut list: Vec<Record<TransactionData>> = self.list(Entity::Transaction)?;
        list.retain(|t| matches_query(&t.data, q));
        list.sort_by_key(|t| std::cmp::Reverse((t.data.occurred_on, t.id)));
        Ok(list)
    }

    pub async fn put_transaction(
        self,
        id: Uuid,
        data: TransactionData,
    ) -> Result<Record<TransactionData>, ApiError> {
        self.put(Entity::Transaction, id, data)
    }

    pub async fn delete_transaction(self, id: Uuid) -> Result<(), ApiError> {
        self.delete(Entity::Transaction, id)
    }

    pub async fn installment_plans(self) -> Result<Vec<Record<InstallmentPlanData>>, ApiError> {
        let mut list: Vec<Record<InstallmentPlanData>> = self.list(Entity::InstallmentPlan)?;
        list.sort_by(|a, b| {
            b.data
                .first_post_on
                .cmp(&a.data.first_post_on)
                .then(a.id.cmp(&b.id))
        });
        Ok(list)
    }

    pub async fn put_installment_plan(
        self,
        id: Uuid,
        data: InstallmentPlanData,
    ) -> Result<Record<InstallmentPlanData>, ApiError> {
        self.put(Entity::InstallmentPlan, id, data)
    }

    pub async fn delete_installment_plan(self, id: Uuid) -> Result<(), ApiError> {
        self.delete(Entity::InstallmentPlan, id)
    }

    pub async fn budgets(self, q: &BudgetQuery) -> Result<Vec<Record<BudgetData>>, ApiError> {
        let mut list: Vec<Record<BudgetData>> = self.list(Entity::Budget)?;
        list.retain(|b| q.period_month.is_none_or(|m| b.data.period_month == m));
        list.sort_by(|a, b| {
            b.data
                .period_month
                .cmp(&a.data.period_month)
                .then(a.id.cmp(&b.id))
        });
        Ok(list)
    }

    pub async fn put_budget(
        self,
        id: Uuid,
        data: BudgetData,
    ) -> Result<Record<BudgetData>, ApiError> {
        self.put(Entity::Budget, id, data)
    }

    pub async fn delete_budget(self, id: Uuid) -> Result<(), ApiError> {
        self.delete(Entity::Budget, id)
    }
}

/// The server's `GET /transactions` filters: account, and an inclusive date range.
fn matches_query(t: &TransactionData, q: &TransactionQuery) -> bool {
    q.account_id.is_none_or(|a| t.account_id == a)
        && q.from.is_none_or(|d| t.occurred_on >= d)
        && q.to.is_none_or(|d| t.occurred_on <= d)
}

/// Waits `ms` milliseconds.
pub async fn sleep(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                    &resolve,
                    i32::try_from(ms).unwrap_or(i32::MAX),
                );
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use billo_api::{Money, TransactionKind};
    use time::macros::date;

    #[test]
    fn status_priorities() {
        let mut info = SyncInfo::default();
        assert_eq!(info.status(0), SyncStatus::Synced);
        assert_eq!(info.status(2), SyncStatus::Waiting(2));
        info.error = Some("x".into());
        assert_eq!(info.status(2), SyncStatus::Error(2));
        info.unreachable = true;
        assert_eq!(info.status(1), SyncStatus::Offline(1));
        info.unreachable = false;
        info.online = false;
        assert_eq!(info.status(0), SyncStatus::Offline(0));
        info.syncing = true;
        assert_eq!(info.status(3), SyncStatus::Syncing);
    }

    #[test]
    fn sign_out_clears_only_when_nothing_is_waiting() {
        let mut info = SyncInfo::default();
        assert_eq!(sign_out_check(&info, 0), SignOutCheck::Clear);
        // A failed push (e.g. server error) leaves changes waiting: ask first.
        info.error = Some("boom".into());
        assert_eq!(
            sign_out_check(&info, 3),
            SignOutCheck::Confirm {
                waiting: 3,
                offline: false
            }
        );
        // Offline: the device has no connection or the server is unreachable.
        info.error = None;
        info.unreachable = true;
        assert_eq!(
            sign_out_check(&info, 1),
            SignOutCheck::Confirm {
                waiting: 1,
                offline: true
            }
        );
        info.unreachable = false;
        info.online = false;
        assert_eq!(
            sign_out_check(&info, 2),
            SignOutCheck::Confirm {
                waiting: 2,
                offline: true
            }
        );
        // Everything went out, even while marked offline: nothing to lose.
        assert_eq!(sign_out_check(&info, 0), SignOutCheck::Clear);
    }

    #[test]
    fn transaction_filters_match_the_server() {
        let account = Uuid::now_v7();
        let t = TransactionData {
            account_id: account,
            category_id: None,
            occurred_on: date!(2026 - 11 - 10),
            posted_on: None,
            amount: Money::new(100, "THB"),
            orig_amount: None,
            kind: TransactionKind::Expense,
            transfer_peer_id: None,
            installment_plan_id: None,
            note: None,
        };
        assert!(matches_query(&t, &TransactionQuery::default()));
        let q = |a, from, to| TransactionQuery {
            account_id: a,
            from,
            to,
        };
        assert!(matches_query(
            &t,
            &q(
                Some(account),
                Some(date!(2026 - 11 - 10)),
                Some(date!(2026 - 11 - 10))
            )
        ));
        assert!(!matches_query(&t, &q(Some(Uuid::nil()), None, None)));
        assert!(!matches_query(
            &t,
            &q(None, Some(date!(2026 - 11 - 11)), None)
        ));
        assert!(!matches_query(
            &t,
            &q(None, None, Some(date!(2026 - 11 - 09)))
        ));
    }
}
