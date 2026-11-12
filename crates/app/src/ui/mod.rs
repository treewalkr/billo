//! The app shell: session, header and screens.

use billo_api::{AccountData, AccountKind, Record};
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{self, ApiError, Client};
use crate::storage::{self, Session};
use crate::strings as s;

mod account_form;
mod accounts;
mod fields;
mod home;
mod signin;

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
        }
    }

    fn tab(&self) -> Tab {
        match self {
            Self::Home => Tab::Home,
            _ => Tab::Accounts,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Accounts,
}

pub fn use_nav() -> Signal<Screen> {
    use_context::<Signal<Screen>>()
}

#[component]
pub fn App() -> Element {
    let session = use_signal(storage::load);
    use_context_provider(|| Auth(session));
    use_context_provider(|| Signal::new(Screen::Home));
    let signed_in = session.read().is_some();
    // Check a saved session once at start-up; a 401 signs out.
    let auth = Auth(session);
    use_resource(move || async move {
        if auth.session().is_some() {
            let _ = auth.call(|c| async move { c.me().await }).await;
        }
    });
    rsx! {
        document::Title { "{s::APP_NAME}" }
        document::Stylesheet { href: MAIN_CSS }
        Header {}
        main {
            if signed_in {
                Screens {}
            } else {
                signin::SignIn {}
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
    };
    rsx! {
        div { key: "{screen.key()}", {body} }
        nav { class: "tabs",
            button {
                id: "tab-home",
                class: if tab == Tab::Home { "active" } else { "" },
                onclick: move |_| nav.set(Screen::Home),
                "{s::THIS_MONTH}"
            }
            button {
                id: "tab-accounts",
                class: if tab == Tab::Accounts { "active" } else { "" },
                onclick: move |_| nav.set(Screen::Accounts),
                "{s::ACCOUNTS}"
            }
        }
    }
}

#[component]
fn Header() -> Element {
    let auth = use_auth();
    let health = use_resource(|| async { Client::new(api::BASE_URL, None).health().await });
    let version = match &*health.read() {
        None => "…".to_owned(),
        Some(Ok(h)) => format!("v{} ({})", h.version, h.build),
        Some(Err(_)) => s::OFFLINE.to_owned(),
    };
    let session = auth.session();
    let sign_out = move |_| {
        spawn(async move {
            // Best effort: the local session goes away even if the server is unreachable.
            let _ = auth.call(|c| async move { c.logout().await }).await;
            auth.forget();
        });
    };
    rsx! {
        header {
            div { class: "brand",
                h1 { "{s::APP_NAME}" }
                span { class: "server", id: "server-version", title: api::BASE_URL, "{s::SERVER} {version}" }
            }
            if let Some(session) = session {
                div { class: "who",
                    span { id: "signed-in-as", title: "{s::SIGNED_IN_AS} {session.email}", "{session.email}" }
                    button { class: "link", id: "sign-out", onclick: sign_out, "{s::SIGN_OUT}" }
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
                button { class: "secondary", onclick: move |_| retry.call(()), "{s::RETRY}" }
            }
        }
    }
}

#[component]
pub fn Loading() -> Element {
    rsx! { p { class: "loading", "{s::LOADING}" } }
}
