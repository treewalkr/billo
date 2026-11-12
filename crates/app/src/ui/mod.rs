//! The app shell: session, header and screens.

use billo_api::{
    AccountData, AccountKind, CategoryData, InstallmentPlanData, Record, TransactionData,
};
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{self, ApiError, Client};
use crate::storage::{self, Session};
use crate::store::{SignOutCheck, Store, SyncMsg, sign_out_check};
use crate::strings::{self as s, Lang};
use crate::web_login::{self, Returned};

mod account_form;
mod accounts;
mod budgets;
mod categories;
mod fields;
mod home;
mod more;
mod plans;
mod signin;
mod sync;
mod transactions;

const MAIN_CSS: Asset = asset!("/assets/main.css");

/// The signed-in session, shared through context. `Copy`, so it can move into closures.
#[derive(Clone, Copy)]
pub struct Auth(Signal<Option<Session>>);

impl Auth {
    pub fn session(&self) -> Option<Session> {
        self.0.read().clone()
    }

    pub fn client(&self) -> Client {
        Client::new(
            api::BASE_URL,
            self.0.read().as_ref().map(|s| s.token.clone()),
        )
    }

    pub fn sign_in(mut self, session: Session) {
        storage::save(&session);
        self.0.set(Some(session));
    }

    /// Forgets the session locally (the server session may already be gone).
    pub fn forget(mut self) {
        storage::clear();
        self.0.set(None);
    }

    /// Runs an API call with the current token; a 401 signs the user out.
    pub async fn call<T, F, Fut>(self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(Client) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let result = f(self.client()).await;
        if matches!(result, Err(ApiError::Unauthorized)) {
            self.forget();
        }
        result
    }
}

pub fn use_auth() -> Auth {
    use_context::<Auth>()
}

/// The `billo_error` code the web Google sign-in came back with, if any.
#[derive(Clone, Copy)]
pub struct GoogleError(pub Signal<Option<String>>);

/// Which screen is showing. Kept in app state (not the URL) for now.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Home,
    Accounts,
    Account(Uuid),
    /// `None`: pick the kind first.
    NewAccount(Option<AccountKind>),
    EditAccount(Box<Record<AccountData>>),
    /// Add (`None`) or edit a transaction on the account.
    TransactionForm(
        Box<Record<AccountData>>,
        Option<Box<Record<TransactionData>>>,
    ),
    /// Add (`None`) or edit an installment plan on the card.
    PlanForm(
        Box<Record<AccountData>>,
        Option<Box<Record<InstallmentPlanData>>>,
    ),
    Budgets,
    More,
    Categories,
    /// Add (`None`) or edit a category.
    CategoryForm(Option<Box<Record<CategoryData>>>),
}

impl Screen {
    /// A distinct key per screen, so forms start fresh when the screen changes.
    fn key(&self) -> String {
        match self {
            Self::Home => "home".into(),
            Self::Accounts => "accounts".into(),
            Self::Account(id) => format!("account-{id}"),
            Self::NewAccount(k) => format!("new-account-{k:?}"),
            Self::EditAccount(r) => format!("edit-account-{}-{}", r.id, r.version),
            Self::TransactionForm(a, t) => match t {
                Some(t) => format!("txn-{}-{}", t.id, t.version),
                None => format!("new-txn-{}", a.id),
            },
            Self::PlanForm(a, p) => match p {
                Some(p) => format!("plan-{}-{}", p.id, p.version),
                None => format!("new-plan-{}", a.id),
            },
            Self::Budgets => "budgets".into(),
            Self::More => "more".into(),
            Self::Categories => "categories".into(),
            Self::CategoryForm(c) => match c {
                Some(c) => format!("category-{}-{}", c.id, c.version),
                None => "new-category".into(),
            },
        }
    }

    fn tab(&self) -> Tab {
        match self {
            Self::Home => Tab::Home,
            Self::Budgets => Tab::Budgets,
            Self::More | Self::Categories | Self::CategoryForm(_) => Tab::More,
            _ => Tab::Accounts,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Accounts,
    Budgets,
    More,
}

pub fn use_nav() -> Signal<Screen> {
    use_context::<Signal<Screen>>()
}

/// The UI language, shared through context. Changing it re-creates the screens so
/// every string is drawn again.
#[derive(Clone, Copy)]
pub struct Language(Signal<Lang>);

impl Language {
    pub fn get(&self) -> Lang {
        *self.0.read()
    }

    pub fn set(mut self, lang: Lang) {
        s::set_lang(lang);
        storage::save_lang(lang);
        self.0.set(lang);
    }
}

pub fn use_language() -> Language {
    use_context::<Language>()
}

/// The user's extra bank holidays, shared through context (device-only, see
/// [`storage::load_holidays`]).
#[derive(Clone, Copy)]
pub struct Holidays(Signal<Vec<time::Date>>);

impl Holidays {
    pub fn get(&self) -> Vec<time::Date> {
        self.0.read().clone()
    }

    /// The calendar due dates shift on: weekends, Thai bank holidays and these.
    pub fn calendar(&self) -> billo_domain::WeekendCalendar {
        billo_domain::WeekendCalendar::thai(self.get())
    }

    pub fn add(mut self, date: time::Date) {
        let mut list = self.0.write();
        if !list.contains(&date) {
            list.push(date);
            list.sort_unstable();
            storage::save_holidays(&list);
        }
    }

    pub fn remove(mut self, date: time::Date) {
        let mut list = self.0.write();
        list.retain(|d| *d != date);
        storage::save_holidays(&list);
    }
}

pub fn use_holidays() -> Holidays {
    use_context::<Holidays>()
}

#[component]
pub fn App() -> Element {
    let language = use_signal(|| {
        let lang = storage::load_lang();
        s::set_lang(lang);
        lang
    });
    use_context_provider(|| Language(language));
    let lang = language();
    // Coming back from the web Google sign-in: keep its session before the saved one
    // is loaded, or hold on to its error for the sign-in screen.
    let returned_error = use_hook(|| match web_login::take() {
        Some(Returned::Session(session)) => {
            storage::save(&session);
            None
        }
        Some(Returned::Error(code)) => Some(code),
        None => None,
    });
    use_context_provider(|| GoogleError(Signal::new(returned_error)));
    let session = use_signal(storage::load);
    use_context_provider(|| Auth(session));
    use_context_provider(|| Signal::new(Screen::Home));
    use_context_provider(|| Holidays(Signal::new(storage::load_holidays())));
    use_context_provider(|| SignOut {
        prompt: Signal::new(None),
        busy: Signal::new(false),
        scope: dioxus::dioxus_core::current_scope_id(),
    });
    let signed_in = session.read().is_some();
    let auth = Auth(session);

    // The local store: loaded once, then kept in step with the signed-in user.
    let store = use_hook(|| {
        let store = Store::create();
        spawn(store.open());
        store
    });
    use_context_provider(|| store);
    sync::use_sync_engine(store, auth);
    use_effect(move || {
        if !store.ready() {
            return;
        }
        match session.read().as_ref().map(|s| s.user_id) {
            Some(user_id) => {
                store.set_user(user_id);
                store.send(SyncMsg::Now);
            }
            // Session gone (sign-out or expired): keep anything not yet synced.
            None => store.clear_if_synced(),
        }
    });
    let ready = store.ready();
    rsx! {
        document::Title { "{s::APP_NAME}" }
        document::Stylesheet { href: MAIN_CSS }
        if cfg!(target_arch = "wasm32") {
            WebHead {}
        }
        // Keyed by language: switching re-creates everything below in the new language.
        for l in [lang] {
            div { key: "{l.code()}", class: "app", lang: l.code(),
                Header {}
                main {
                    if signed_in && !ready {
                        Loading {}
                    } else if signed_in {
                        sync::SyncNotices {}
                        Screens {}
                    } else {
                        signin::SignIn {}
                    }
                }
                SignOutDialog {}
            }
        }
    }
}

/// The web app's manifest and service worker (nothing on iOS / Android).
#[component]
fn WebHead() -> Element {
    #[cfg(target_arch = "wasm32")]
    return rsx! { crate::pwa::WebAppHead {} };
    #[cfg(not(target_arch = "wasm32"))]
    rsx! {}
}

#[component]
fn Screens() -> Element {
    let mut nav = use_nav();
    let screen = nav.read().clone();
    let tab = screen.tab();
    let body = match screen.clone() {
        Screen::Home => rsx! { home::Home {} },
        Screen::Accounts => rsx! { accounts::Accounts {} },
        Screen::Account(id) => rsx! { accounts::AccountPage { id } },
        Screen::NewAccount(None) => rsx! { account_form::PickKind {} },
        Screen::NewAccount(Some(kind)) => rsx! { account_form::AccountForm { kind } },
        Screen::EditAccount(r) => {
            rsx! { account_form::AccountForm { kind: r.data.kind, existing: *r } }
        }
        Screen::TransactionForm(a, t) => rsx! {
            transactions::TransactionForm { account: *a, existing: t.map(|t| *t) }
        },
        Screen::PlanForm(a, p) => rsx! {
            plans::PlanForm { account: *a, existing: p.map(|p| *p) }
        },
        Screen::Budgets => rsx! { budgets::Budgets {} },
        Screen::More => rsx! { more::More {} },
        Screen::Categories => rsx! { categories::Categories {} },
        Screen::CategoryForm(c) => rsx! { categories::CategoryForm { existing: c.map(|c| *c) } },
    };
    rsx! {
        div { key: "{screen.key()}", {body} }
        nav { class: "tabs",
            button {
                id: "tab-home",
                class: if tab == Tab::Home { "active" } else { "" },
                onclick: move |_| nav.set(Screen::Home),
                "{s::this_month()}"
            }
            button {
                id: "tab-accounts",
                class: if tab == Tab::Accounts { "active" } else { "" },
                onclick: move |_| nav.set(Screen::Accounts),
                "{s::accounts()}"
            }
            button {
                id: "tab-budgets",
                class: if tab == Tab::Budgets { "active" } else { "" },
                onclick: move |_| nav.set(Screen::Budgets),
                "{s::budgets()}"
            }
            button {
                id: "tab-more",
                class: if tab == Tab::More { "active" } else { "" },
                onclick: move |_| nav.set(Screen::More),
                "{s::more()}"
            }
        }
    }
}

/// App name and sync status only: the account and versions live in More, so nothing
/// here wraps on a narrow phone.
#[component]
fn Header() -> Element {
    let auth = use_auth();
    rsx! {
        header {
            h1 { "{s::APP_NAME}" }
            if auth.session().is_some() {
                sync::SyncIndicator {}
            }
        }
    }
}

/// Sign-out in progress, and the "changes not synced" question. Shared through context.
#[derive(Clone, Copy)]
pub struct SignOut {
    prompt: Signal<Option<SignOutCheck>>,
    busy: Signal<bool>,
    /// The root component's scope: sign-out work runs there, so it isn't cancelled
    /// when the screen that started it goes away.
    scope: ScopeId,
}

impl SignOut {
    pub fn busy(&self) -> bool {
        *self.busy.read()
    }

    fn spawn(&self, fut: impl Future<Output = ()> + 'static) {
        dioxus::dioxus_core::Runtime::current().spawn(self.scope, fut);
    }
}

pub fn use_sign_out() -> SignOut {
    use_context::<SignOut>()
}

/// Tries to push waiting changes first. Local data is cleared only if nothing is left
/// waiting; otherwise the user is asked (see [`SignOutDialog`]).
pub fn sign_out(auth: Auth, store: Store) {
    let ctx = consume_context::<SignOut>();
    let SignOut {
        mut prompt,
        mut busy,
        ..
    } = ctx;
    if *busy.peek() {
        return;
    }
    busy.set(true);
    ctx.spawn(async move {
        if store.pending_now() > 0 {
            let _ = store.sync(auth.client()).await;
        }
        match sign_out_check(&store.info(), store.pending_now()) {
            SignOutCheck::Clear => end_session(auth).await,
            check @ SignOutCheck::Confirm { .. } => prompt.set(Some(check)),
        }
        busy.set(false);
    });
}

/// Ends the session on the server (best effort), then forgets it; the root clears the
/// local data when the session goes.
async fn end_session(auth: Auth) {
    // The local session goes away even if the server is unreachable.
    let _ = auth.call(|c| async move { c.logout().await }).await;
    auth.forget();
}

/// Asks before discarding changes that couldn't be sent. "Stay signed in" is the default.
#[component]
fn SignOutDialog() -> Element {
    let auth = use_auth();
    let store = crate::store::use_store();
    let ctx = use_sign_out();
    let mut prompt = ctx.prompt;
    let Some(SignOutCheck::Confirm { waiting, offline }) = prompt() else {
        return rsx! {};
    };
    rsx! {
        div { class: "dialog-backdrop",
            div {
                class: "dialog",
                id: "sign-out-dialog",
                role: "alertdialog",
                "aria-modal": "true",
                "aria-labelledby": "sign-out-dialog-title",
                "aria-describedby": "sign-out-dialog-text",
                h3 { id: "sign-out-dialog-title", "{s::unsynced_title()}" }
                p { id: "sign-out-dialog-text", "{s::unsynced_on_sign_out(waiting, offline)}" }
                div { class: "actions",
                    button {
                        id: "stay-signed-in",
                        autofocus: true,
                        onmounted: move |e| async move {
                            let _ = e.set_focus(true).await;
                        },
                        onclick: move |_| prompt.set(None),
                        "{s::stay_signed_in()}"
                    }
                    button {
                        class: "danger",
                        id: "sign-out-discard",
                        onclick: move |_| {
                            prompt.set(None);
                            // The user chose to discard: clear even what is waiting.
                            store.clear();
                            ctx.spawn(end_session(auth));
                        },
                        "{s::sign_out_discard()}"
                    }
                }
            }
        }
    }
}

/// Thai / English buttons.
#[component]
pub fn LanguageSwitch() -> Element {
    let language = use_language();
    let current = language.get();
    rsx! {
        div { class: "lang-switch", role: "group", "aria-label": s::language(),
            for l in Lang::ALL {
                button {
                    id: "lang-{l.code()}",
                    class: if l == current { "active" } else { "" },
                    "aria-pressed": l == current,
                    onclick: move |_| language.set(l),
                    "{l.own_name()}"
                }
            }
        }
    }
}

/// An error for the user, with the raw error (if any) for a "Details" line.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub message: String,
    pub details: Option<String>,
}

impl From<String> for Problem {
    fn from(message: String) -> Self {
        Self {
            message,
            details: None,
        }
    }
}

impl From<&str> for Problem {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<ApiError> for Problem {
    fn from(e: ApiError) -> Self {
        Self {
            message: e.to_string(),
            details: e.details(),
        }
    }
}

/// An error message with an optional retry button, and the raw error behind a
/// "Details" toggle.
#[component]
pub fn ErrorBox(
    message: String,
    #[props(default)] details: Option<String>,
    on_retry: Option<EventHandler<()>>,
) -> Element {
    rsx! {
        div { class: "error-box", role: "alert",
            p { "{message}" }
            if let Some(details) = details {
                details { class: "error-details",
                    summary { "{s::error_details()}" }
                    code { "{details}" }
                }
            }
            if let Some(retry) = on_retry {
                button { class: "secondary", onclick: move |_| retry.call(()), "{s::retry()}" }
            }
        }
    }
}

#[component]
pub fn Loading() -> Element {
    rsx! { p { class: "loading", "{s::loading()}" } }
}
