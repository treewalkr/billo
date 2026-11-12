//! "More": categories, sync, language, theme, the signed-in account, and versions.

use dioxus::prelude::*;

use super::sync::SyncCard;
use super::{
    ErrorBox, LanguageSwitch, Problem, Screen, ThemeSwitch, sign_out, use_auth, use_holidays,
    use_nav, use_sign_out,
};
use crate::api::{self, Client};
use crate::store::use_store;
use crate::{format, input, strings as s};

#[component]
pub fn More() -> Element {
    let auth = use_auth();
    let store = use_store();
    let mut nav = use_nav();
    let busy = use_sign_out().busy();
    let email = auth.session().map(|s| s.email).unwrap_or_default();
    rsx! {
        h2 { "{s::more()}" }
        section { class: "card",
            ul { class: "list",
                li { class: "clickable",
                    button {
                        class: "row-button",
                        id: "open-categories",
                        onclick: move |_| nav.set(Screen::Categories),
                        span { class: "main",
                            span { class: "title", "{s::categories()}" }
                        }
                        span { class: "sub", "aria-hidden": "true", "›" }
                    }
                }
            }
        }
        SyncCard {}
        HolidaysCard {}
        section { class: "card", id: "language",
            div { class: "card-head",
                h3 { "{s::language()}" }
                LanguageSwitch {}
            }
        }
        section { class: "card", id: "theme",
            div { class: "card-head",
                h3 { "{s::theme()}" }
                ThemeSwitch {}
            }
        }
        section { class: "card", id: "account",
            h3 { "{s::account_title()}" }
            div { class: "card-head",
                div { class: "main",
                    span { class: "sub", "{s::signed_in_as()}" }
                    span { class: "title", id: "signed-in-as", "{email}" }
                }
                button { class: "secondary", id: "more-sign-out", disabled: busy, onclick: move |_| sign_out(auth, store),
                    if busy { "{s::signing_out()}" } else { "{s::sign_out()}" }
                }
            }
        }
        About {}
    }
}

/// This app's own version (from the workspace `Cargo.toml`) and the server's.
fn app_version() -> String {
    let build = env!("BILLO_BUILD")
        .parse()
        .expect("BILLO_BUILD is set by build.rs to a u32");
    s::version_build(env!("CARGO_PKG_VERSION"), build)
}

#[component]
fn About() -> Element {
    let store = use_store();
    // Checked again whenever the connection or the server's reachability changes.
    let reach = use_memo(move || {
        let info = store.info();
        (info.online, info.unreachable)
    });
    let health = use_resource(move || async move {
        let _ = reach();
        Client::new(api::BASE_URL, None).health().await
    });
    let server = match &*health.read() {
        None => "…".to_owned(),
        Some(Ok(h)) => s::version_build(&h.version, h.build),
        Some(Err(_)) => s::offline().to_owned(),
    };
    rsx! {
        section { class: "card", id: "about",
            h3 { "{s::about()}" }
            dl {
                div { class: "row",
                    dt { "{s::app_label()}" }
                    dd { id: "app-version", "{app_version()}" }
                }
                div { class: "row",
                    dt { "{s::server()}" }
                    dd { id: "server-version", title: api::BASE_URL, "{server}" }
                }
            }
        }
    }
}

/// Built-in Thai bank holidays, and the user's own extra days.
#[component]
fn HolidaysCard() -> Element {
    let holidays = use_holidays();
    let list = holidays.get();
    let mut text = use_signal(String::new);
    let mut error = use_signal(|| None::<Problem>);
    let add = move |e: FormEvent| {
        e.prevent_default();
        match input::field(s::holiday_date(), input::parse_date(&text())) {
            Ok(d) => {
                holidays.add(d);
                text.set(String::new());
                error.set(None);
            }
            Err(msg) => error.set(Some(msg.into())),
        }
    };
    rsx! {
        section { class: "card", id: "holidays",
            h3 { "{s::holidays_title()}" }
            p { class: "note", "{s::holidays_note()}" }
            h4 { "{s::extra_holidays()}" }
            if list.is_empty() {
                p { class: "note", id: "no-extra-holidays", "{s::no_extra_holidays()}" }
            } else {
                ul { class: "list", id: "extra-holidays",
                    for d in list {
                        li { key: "{d}",
                            div { class: "main",
                                span { class: "title", "{format::date(d)}" }
                            }
                            button {
                                class: "link",
                                "aria-label": "{s::remove()} {format::date(d)}",
                                onclick: move |_| holidays.remove(d),
                                "{s::remove()}"
                            }
                        }
                    }
                }
            }
            form { class: "inline-add", onsubmit: add,
                label { r#for: "holiday-date", class: "sr-only", "{s::holiday_date()}" }
                input {
                    id: "holiday-date",
                    r#type: "date",
                    value: "{text}",
                    oninput: move |e| text.set(e.value()),
                }
                button { r#type: "submit", class: "secondary", id: "add-holiday", "{s::add()}" }
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg.message, details: msg.details }
            }
            p { class: "note", "{s::holidays_device_only()}" }
        }
    }
}
