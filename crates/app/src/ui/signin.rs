//! Development sign-in: email only, via `POST /auth/dev-login`.

use dioxus::prelude::*;

use super::{ErrorBox, LanguageSwitch, Problem, use_auth};
use crate::api::ApiError;
use crate::storage::Session;
use crate::strings as s;

#[component]
pub fn SignIn() -> Element {
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
                    email: address.to_lowercase(),
                }),
                Err(ApiError::NotFound) => error.set(Some(s::dev_login_off().into())),
                Err(e) => error.set(Some(e.into())),
            }
            busy.set(false);
        });
    };

    rsx! {
        section { class: "card signin",
            div { class: "card-head",
                h2 { "{s::sign_in()}" }
                LanguageSwitch {}
            }
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
}
