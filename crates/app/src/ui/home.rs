//! The "This month" screen.

use dioxus::prelude::*;

use crate::{demo, format};

#[component]
pub fn Home() -> Element {
    rsx! {
        h2 { "This month" }
        DemoCard {}
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
