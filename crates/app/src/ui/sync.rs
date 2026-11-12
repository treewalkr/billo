//! The sync engine's schedule, and its status in the UI.
//!
//! Runs a sync on start-up and sign-in, ~1 s after the last local write, when the
//! browser comes back online, on "Sync now", every few minutes while idle, and after a
//! failure with a growing delay (2 s → 60 s).

use std::pin::pin;

use dioxus::prelude::*;
use futures_util::StreamExt;
use futures_util::future::{Either, select};

use super::Auth;
use crate::api::ApiError;
use crate::store::{Store, SyncMsg, SyncStatus, sleep, use_store};
use crate::strings as s;

/// Quiet period after a local write before pushing.
const DEBOUNCE_MS: u32 = 1_000;
/// Pull for other devices' changes this often while idle.
const IDLE_MS: u32 = 5 * 60 * 1_000;
const RETRY_MIN_MS: u32 = 2_000;
const RETRY_MAX_MS: u32 = 60_000;

enum Wake {
    Msg(SyncMsg),
    Timeout,
    Closed,
}

async fn next(rx: &mut UnboundedReceiver<SyncMsg>, ms: u32) -> Wake {
    let msg = rx.next();
    let timer = pin!(sleep(ms));
    match select(msg, timer).await {
        Either::Left((Some(m), _)) => Wake::Msg(m),
        Either::Left((None, _)) => Wake::Closed,
        Either::Right(_) => Wake::Timeout,
    }
}

/// Starts the engine; call once in the root component.
pub fn use_sync_engine(store: Store, auth: Auth) {
    let engine = use_coroutine(move |rx: UnboundedReceiver<SyncMsg>| run(store, auth, rx));
    use_hook(|| {
        store.set_engine(engine);
        #[cfg(target_arch = "wasm32")]
        watch_connectivity(engine.tx());
    });
}

async fn run(store: Store, auth: Auth, mut rx: UnboundedReceiver<SyncMsg>) {
    let mut retry_ms: u32 = 0;
    loop {
        let wait = if retry_ms > 0 { retry_ms } else { IDLE_MS };
        match next(&mut rx, wait).await {
            Wake::Closed => return,
            Wake::Msg(SyncMsg::Online(online)) => {
                store.set_online(online);
                if !online {
                    continue;
                }
            }
            Wake::Msg(SyncMsg::Changed) => {
                // Let a burst of writes settle.
                loop {
                    match next(&mut rx, DEBOUNCE_MS).await {
                        Wake::Msg(SyncMsg::Changed) => {}
                        Wake::Msg(SyncMsg::Online(online)) => store.set_online(online),
                        Wake::Msg(SyncMsg::Now) | Wake::Timeout => break,
                        Wake::Closed => return,
                    }
                }
            }
            Wake::Msg(SyncMsg::Now) | Wake::Timeout => {}
        }
        if !store.ready() || auth.session().is_none() || !store.info().online {
            continue;
        }
        match store.sync(auth.client()).await {
            // Writes made during the sync go out right after.
            Ok(_) => {
                retry_ms = if store.pending_now() > 0 {
                    DEBOUNCE_MS
                } else {
                    0
                }
            }
            Err(ApiError::Unauthorized) => {
                auth.forget();
                retry_ms = 0;
            }
            Err(_) => retry_ms = retry_ms.saturating_mul(2).clamp(RETRY_MIN_MS, RETRY_MAX_MS),
        }
    }
}

/// Web: follow the browser's `online` / `offline` events.
#[cfg(target_arch = "wasm32")]
fn watch_connectivity(tx: UnboundedSender<SyncMsg>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = tx.unbounded_send(SyncMsg::Online(window.navigator().on_line()));
    for (event, online) in [("online", true), ("offline", false)] {
        let tx = tx.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let _ = tx.unbounded_send(SyncMsg::Online(online));
        });
        let _ = window.add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
        // Lives as long as the page.
        cb.forget();
    }
}

fn status_text(status: SyncStatus) -> (&'static str, String) {
    match status {
        SyncStatus::Synced => ("synced", s::sync_synced().to_owned()),
        SyncStatus::Syncing => ("syncing", s::sync_syncing().to_owned()),
        SyncStatus::Waiting(n) => ("waiting", s::changes_waiting(n)),
        SyncStatus::Offline(0) => ("offline", s::offline_label().to_owned()),
        SyncStatus::Offline(n) => (
            "offline",
            format!("{} · {}", s::offline_label(), s::changes_waiting(n)),
        ),
        SyncStatus::Error(0) => ("error", s::sync_failed().to_owned()),
        SyncStatus::Error(n) => (
            "error",
            format!("{} · {}", s::sync_failed(), s::changes_waiting(n)),
        ),
    }
}

/// A small status pill: synced / syncing / N waiting / offline / error.
#[component]
pub fn SyncIndicator() -> Element {
    let store = use_store();
    let status = store.status();
    let (state, text) = status_text(status);
    let title = store
        .info()
        .error
        .map(|e| match e.details() {
            Some(d) => format!("{e} ({d})"),
            None => e.to_string(),
        })
        .unwrap_or_default();
    rsx! {
        span {
            class: "sync-status {state}",
            id: "sync-status",
            "data-state": state,
            title: "{title}",
            role: "status",
            "{text}"
        }
    }
}

/// Quiet notices about what the last syncs did.
#[component]
pub fn SyncNotices() -> Element {
    let store = use_store();
    let info = store.info();
    if info.conflicts == 0 && info.rejected == 0 && !info.store_failed {
        return rsx! {};
    }
    rsx! {
        div { class: "sync-notice", id: "sync-notice", role: "status",
            if info.store_failed {
                p { "{s::local_store_failed()}" }
            }
            if info.conflicts > 0 {
                p { id: "conflict-notice", "{s::conflict_notice()}" }
            }
            if info.rejected > 0 {
                p { id: "rejected-notice", "{s::rejected_notice(info.rejected)}" }
                if let Some(e) = info.rejection.clone() {
                    p { id: "rejected-reason", "{e}" }
                    if let Some(d) = e.details() {
                        details { class: "error-details",
                            summary { "{s::error_details()}" }
                            code { "{d}" }
                        }
                    }
                }
            }
            if info.conflicts > 0 || info.rejected > 0 {
                button { class: "link", id: "dismiss-sync-notice", onclick: move |_| store.dismiss_notices(), "{s::dismiss()}" }
            }
        }
    }
}

/// The "Sync" card in More.
#[component]
pub fn SyncCard() -> Element {
    let store = use_store();
    let info = store.info();
    let syncing = info.syncing;
    rsx! {
        section { class: "card", id: "sync",
            div { class: "card-head",
                div { class: "main",
                    h3 { "{s::sync_title()}" }
                    SyncIndicator {}
                }
                button {
                    class: "secondary",
                    id: "sync-now",
                    disabled: syncing,
                    onclick: move |_| store.send(SyncMsg::Now),
                    "{s::sync_now()}"
                }
            }
            if let (Some(e), false) = (info.error, syncing) {
                div { id: "sync-error",
                    super::ErrorBox { message: e.to_string(), details: e.details() }
                }
            }
        }
    }
}
