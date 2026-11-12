//! The sign-in screen: "Sign in with Google" (web) and, on development servers, the
//! email-only dev sign-in via `POST /auth/dev-login`. What's shown depends on the
//! server's `GET /auth/methods`.

use billo_api::AuthMethods;
use dioxus::prelude::*;

use super::{ErrorBox, GoogleError, LanguageSwitch, Loading, Problem, use_auth};
use crate::api::ApiError;
use crate::storage::{self, Session};
use crate::strings as s;
use crate::web_login;

/// The Google button is web-only for now. Next: native sign-in on iOS / Android
/// (ASWebAuthenticationSession or the platform SDK) posting the ID token to
/// `POST /auth/google`.
const GOOGLE_ON_THIS_PLATFORM: bool = cfg!(target_arch = "wasm32");

#[component]
pub fn SignIn() -> Element {
    let auth = use_auth();
    // Last known methods first, so the screen shows at once (and offline); then ask
    // the server again.
    let saved = use_hook(storage::load_auth_methods);
    let mut fetched = use_resource(move || async move {
        let result = auth.client().auth_methods().await;
        if let Ok(m) = &result {
            storage::save_auth_methods(m);
        }
        result
    });
    let GoogleError(google_error) = use_context::<GoogleError>();

    let (current, problem) = match &*fetched.read() {
        Some(Ok(m)) => (Some(*m), None),
        Some(Err(e)) => (saved, Some(Problem::from(e.clone()))),
        None => (saved, None),
    };
    let body = match current {
        None if problem.is_none() => rsx! { Loading {} },
        None => rsx! {},
        Some(AuthMethods {
            google_sign_in,
            dev_login,
        }) => {
            let google = google_sign_in && GOOGLE_ON_THIS_PLATFORM;
            rsx! {
                if google {
                    GoogleButton {}
                }
                if google && dev_login {
                    div { class: "divider", role: "separator", span { "{s::or_divider()}" } }
                }
                if dev_login {
                    DevLogin {}
                }
                if !google && !dev_login {
                    p { class: "note", id: "no-sign-in", "{s::no_sign_in_methods()}" }
                }
            }
        }
    };

    rsx! {
        section { class: "card signin",
            div { class: "card-head",
                h2 { "{s::sign_in()}" }
                LanguageSwitch {}
            }
            if let Some(code) = google_error() {
                div { id: "google-error",
                    ErrorBox {
                        message: s::google_error_text(&code).to_owned(),
                        details: Some(format!("google: {code}")),
                    }
                }
            }
            if let Some(p) = problem {
                div { id: "methods-error",
                    ErrorBox {
                        message: p.message,
                        details: p.details,
                        on_retry: move |()| fetched.restart(),
                    }
                }
            }
            {body}
        }
    }
}

/// White button with Google's "G", per Google's branding guidance.
#[component]
fn GoogleButton() -> Element {
    let auth = use_auth();
    let GoogleError(mut google_error) = use_context::<GoogleError>();
    let mut leaving = use_signal(|| false);
    rsx! {
        button {
            r#type: "button",
            class: "google",
            id: "sign-in-google",
            disabled: leaving(),
            onclick: move |_| {
                let Some(origin) = web_login::current_origin() else { return };
                google_error.set(None);
                leaving.set(true);
                web_login::navigate(&auth.client().google_start_url(&origin));
            },
            GoogleMark {}
            span { "{s::sign_in_google()}" }
        }
    }
}

#[component]
fn GoogleMark() -> Element {
    rsx! {
        svg {
            class: "google-mark",
            view_box: "0 0 48 48",
            width: "18",
            height: "18",
            "aria-hidden": "true",
            path {
                fill: "#EA4335",
                d: "M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z",
            }
            path {
                fill: "#4285F4",
                d: "M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z",
            }
            path {
                fill: "#FBBC05",
                d: "M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z",
            }
            path {
                fill: "#34A853",
                d: "M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z",
            }
        }
    }
}

/// Development sign-in: email only.
#[component]
fn DevLogin() -> Element {
    let auth = use_auth();
    let mut email = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<Problem>);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let address = email.read().trim().to_owned();
        if address.is_empty() {
            error.set(Some(s::email_required().into()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            match auth.client().dev_login(&address).await {
                Ok(login) => auth.sign_in(Session {
                    token: login.token,
                    user_id: login.user_id,
                    email: login.email.unwrap_or_else(|| address.to_lowercase()),
                }),
                Err(ApiError::NotFound) => error.set(Some(s::dev_login_off().into())),
                Err(e) => error.set(Some(e.into())),
            }
            busy.set(false);
        });
    };

    rsx! {
        p { class: "note", "{s::dev_sign_in_note()}" }
        form { onsubmit: submit,
            label { r#for: "email", "{s::email()}" }
            input {
                id: "email",
                r#type: "email",
                autocomplete: "email",
                placeholder: s::email_placeholder(),
                value: "{email}",
                oninput: move |e| email.set(e.value()),
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg.message, details: msg.details }
            }
            button { r#type: "submit", id: "sign-in", disabled: busy(),
                if busy() { "{s::signing_in()}" } else { "{s::sign_in()}" }
            }
        }
    }
}
