//! The app shell: session, header and screens.

use billo_api::{
    AccountData, AccountKind, CategoryData, InstallmentPlanData, Record, TransactionData,
};
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{self, ApiError, Client};
use crate::storage::{self, Session};
use crate::store::{Store, SyncMsg, use_store};
use crate::strings::{self as s, Lang};

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

#[component]
pub fn App() -> Element {
    let language = use_signal(|| {
        let lang = storage::load_lang();
        s::set_lang(lang);
        lang
    });
    use_context_provider(|| Language(language));
    let lang = language();
    let session = use_signal(storage::load);
    use_context_provider(|| Auth(session));
    use_context_provider(|| Signal::new(Screen::Home));
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
            None => store.clear(),
        }
    });
    let ready = store.ready();
    rsx! {
        document::Title { "{s::APP_NAME}" }
        document::Stylesheet { href: MAIN_CSS }
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
            }
        }
    }
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

#[component]
fn Header() -> Element {
    let auth = use_auth();
    let store = use_store();
    // Checked again whenever the server becomes reachable or unreachable.
    let unreachable = use_memo(move || store.info().unreachable);
    let health = use_resource(move || async move {
        let _ = unreachable();
        Client::new(api::BASE_URL, None).health().await
    });
    let version = match &*health.read() {
        None => "…".to_owned(),
        Some(Ok(h)) => format!("v{} ({})", h.version, h.build),
        Some(Err(_)) => s::offline().to_owned(),
    };
    let session = auth.session();
    let sign_out = move |_| sign_out(auth, store);
    rsx! {
        header {
            div { class: "brand",
                h1 { "{s::APP_NAME}" }
                span { class: "server", id: "server-version", title: api::BASE_URL, "{s::server()} {version}" }
            }
            if let Some(session) = session {
                div { class: "who",
                    sync::SyncIndicator {}
                    span { id: "signed-in-as", title: "{s::signed_in_as()} {session.email}", "{session.email}" }
                    button { class: "link", id: "sign-out", onclick: sign_out, "{s::sign_out()}" }
                }
            }
        }
    }
}

/// Pushes waiting changes and ends the session on the server (both best effort), then
/// forgets the session and the local data.
pub fn sign_out(auth: Auth, store: Store) {
    spawn(async move {
        if store.pending_now() > 0 {
            let _ = store.sync(auth.client()).await;
        }
        // The local session goes away even if the server is unreachable.
        let _ = auth.call(|c| async move { c.logout().await }).await;
        auth.forget();
    });
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

/// An error message with an optional retry button.
#[component]
pub fn ErrorBox(message: String, on_retry: Option<EventHandler<()>>) -> Element {
    rsx! {
        div { class: "error-box", role: "alert",
            p { "{message}" }
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
