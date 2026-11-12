//! "More": categories, sync, language, and the signed-in account.

use dioxus::prelude::*;

use super::sync::SyncCard;
use super::{LanguageSwitch, Screen, sign_out, use_auth, use_nav};
use crate::store::use_store;
use crate::strings as s;

#[component]
pub fn More() -> Element {
    let auth = use_auth();
    let store = use_store();
    let mut nav = use_nav();
    let email = auth.session().map(|s| s.email).unwrap_or_default();
    rsx! {
        h2 { "{s::more()}" }
        section { class: "card",
            ul { class: "list",
                li {
                    class: "clickable",
                    id: "open-categories",
                    onclick: move |_| nav.set(Screen::Categories),
                    div { class: "main",
                        span { class: "title", "{s::categories()}" }
                    }
                    span { class: "sub", "›" }
                }
            }
        }
        SyncCard {}
        section { class: "card", id: "language",
            div { class: "card-head",
                h3 { "{s::language()}" }
                LanguageSwitch {}
            }
        }
        section { class: "card",
            div { class: "card-head",
                div { class: "main",
                    span { class: "sub", "{s::signed_in_as()}" }
                    span { class: "title", id: "more-email", "{email}" }
                }
                button { class: "secondary", id: "more-sign-out", onclick: move |_| sign_out(auth, store), "{s::sign_out()}" }
            }
        }
    }
}
