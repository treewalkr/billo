//! The "This month" screen.

use dioxus::prelude::*;

use crate::{api, demo, format};

const MAIN_CSS: Asset = asset!("/assets/main.css");

#[component]
pub fn App() -> Element {
    rsx! {
        document::Title { "Billo" }
        document::Stylesheet { href: MAIN_CSS }
        Header {}
        main {
            h2 { "This month" }
            DemoCard {}
        }
    }
}

#[component]
fn Header() -> Element {
    let health = use_resource(api::health);
    let version = match &*health.read() {
        None => "…".to_owned(),
        Some(Ok(h)) => format!("v{} ({})", h.version, h.build),
        Some(Err(_)) => "offline".to_owned(),
    };
    rsx! {
        header {
            h1 { "Billo" }
            span { class: "server", id: "server-version", title: api::BASE_URL, "server {version}" }
        }
    }
}

#[component]
fn DemoCard() -> Element {
    let d = match demo::november() {
        Ok(d) => d,
        Err(e) => return rsx! { p { class: "error", "Could not build the sample statement: {e}" } },
    };
    let s = d.statement;
    let rows = [
        ("Statement date", format::date(s.period.statement_date)),
        ("Due date", format::date(s.period.due_date)),
        ("Opening balance", format::money(s.opening_balance)),
        ("New charges", format::money(s.charges)),
        ("Payments", format::money(s.credits)),
        ("Interest", format::money(s.interest)),
        ("Closing balance", format::money(s.closing_balance)),
        ("Minimum due", format::money(s.minimum_due)),
    ];
    rsx! {
        section { class: "card", id: "demo-statement",
            div { class: "card-head",
                h3 { "{demo::CARD_NAME}" }
                span { class: "badge", "Sample data" }
            }
            dl {
                for (label, value) in rows {
                    div { class: "row",
                        dt { "{label}" }
                        dd { "{value}" }
                    }
                }
            }
            p { class: "note",
                "Includes {format::money(d.installment_portion)} from {d.installment_name} "
                "(1 of {d.installment_months}), due in full."
            }
        }
    }
}
