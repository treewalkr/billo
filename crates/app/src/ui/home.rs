//! "This month": every card statement and loan payment, sorted by due date.

use billo_api::TransactionQuery;
use dioxus::prelude::*;

use super::{ErrorBox, Loading, Screen, use_auth, use_nav};
use crate::api::ApiError;
use crate::overview::{self, CardDue, Detail, Item, LoanDue, Status, ThisMonth};
use crate::{clock, format, strings as s};

#[component]
pub fn Home() -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let mut data = use_resource(move || async move {
        let (accounts, txns, plans) = auth
            .call(|c| async move {
                let accounts = c.accounts().await?;
                let txns = c.transactions(&TransactionQuery::default()).await?;
                let plans = c.installment_plans().await?;
                Ok((accounts, txns, plans))
            })
            .await?;
        let today = clock::today();
        Ok::<_, ApiError>((
            today,
            accounts.is_empty(),
            overview::this_month(&accounts, &txns, &plans, today),
        ))
    });

    let body = match &*data.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), on_retry: move |_| data.restart() }
        },
        Some(Ok((_, true, _))) => rsx! {
            div { class: "empty",
                p { "{s::HOME_EMPTY}" }
                button { onclick: move |_| nav.set(Screen::NewAccount(None)), "{s::ADD_ACCOUNT}" }
            }
        },
        Some(Ok((today, false, month))) => {
            let ThisMonth { items, errors } = month.clone();
            let nothing = items.is_empty() && errors.is_empty();
            rsx! {
                p { class: "note today", "{s::TODAY} {format::date(*today)}" }
                for e in errors {
                    section { class: "card item-error", key: "err-{e.account_id}",
                        div { class: "card-head",
                            h3 { "{e.account_name}" }
                        }
                        ErrorBox { message: format!("{}: {}", s::CANT_WORK_OUT, e.message) }
                    }
                }
                for item in items {
                    ItemCard { item }
                }
                if nothing {
                    p { class: "empty", "{s::NOTHING_DUE}" }
                }
            }
        }
    };

    rsx! {
        h2 { "{s::THIS_MONTH}" }
        {body}
    }
}

fn status_badge(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::NothingDue => ("badge", s::STATUS_NOTHING_DUE),
        Status::PaidInFull => ("badge paid", s::STATUS_PAID_IN_FULL),
        Status::Paid => ("badge paid", s::STATUS_PAID),
        Status::MinimumPaid => ("badge paid", s::STATUS_MINIMUM_PAID),
        Status::Unpaid => ("badge unpaid", s::STATUS_UNPAID),
        Status::Overdue => ("badge overdue", s::STATUS_OVERDUE),
        Status::Open => ("badge", s::STATUS_OPEN),
    }
}

#[component]
fn ItemCard(item: Item) -> Element {
    let mut nav = use_nav();
    let id = item.account_id;
    let (subtitle, status, rows, note) = match &item.detail {
        Detail::Card(d) => card_rows(d),
        Detail::Loan(d) => loan_rows(d),
    };
    let (badge_class, badge) = status_badge(status);
    let kind = match item.detail {
        Detail::Card(_) => "card-item",
        Detail::Loan(_) => "loan-item",
    };
    rsx! {
        section {
            class: "card due {kind}",
            key: "{item.account_id}-{item.due_date}",
            "data-account": "{item.account_name}",
            "data-status": "{status:?}",
            div { class: "card-head",
                div {
                    h3 {
                        button { class: "link", onclick: move |_| nav.set(Screen::Account(id)), "{item.account_name}" }
                    }
                    span { class: "sub", "{subtitle}" }
                }
                span { class: badge_class, "{badge}" }
            }
            dl {
                for (label, value, strong) in rows {
                    div {
                        class: if strong { "row strong" } else { "row" },
                        "data-label": label,
                        dt { "{label}" }
                        dd { "{value}" }
                    }
                }
            }
            if let Some(n) = note {
                p { class: "note", "{n}" }
            }
        }
    }
}

type Rows = Vec<(&'static str, String, bool)>;

fn card_rows(d: &CardDue) -> (String, Status, Rows, Option<String>) {
    let st = &d.statement;
    let open = d.status == Status::Open
        || (d.status == Status::NothingDue && st.period.statement_date >= clock::today());
    let subtitle = if open {
        format!(
            "{} {}",
            s::NEXT_STATEMENT,
            format::date(st.period.statement_date)
        )
    } else {
        format!(
            "{} {}",
            s::STATEMENT_OF,
            format::date(st.period.statement_date)
        )
    };
    let mut rows: Rows = vec![
        (s::DUE_DATE, format::date(st.period.due_date), true),
        (s::OPENING_BALANCE, format::money(st.opening_balance), false),
        (s::NEW_CHARGES, format::money(st.charges), false),
        (s::PAYMENTS_AND_CREDITS, format::money(st.credits), false),
        (s::INTEREST, format::money(st.interest), false),
    ];
    if open {
        rows.push((s::BALANCE_SO_FAR, format::money(st.closing_balance), true));
        rows.push((s::MINIMUM_SO_FAR, format::money(st.minimum_due), true));
    } else {
        rows.push((s::CLOSING_BALANCE, format::money(st.closing_balance), true));
        rows.push((s::MINIMUM_DUE, format::money(st.minimum_due), true));
        rows.push((s::PAID_SINCE_STATEMENT, format::money(d.paid), false));
    }
    if st.overdue.minor() > 0 {
        rows.push((s::OVERDUE_FROM_LAST, format::money(st.overdue), false));
    }
    let mut notes = Vec::new();
    if d.installments.minor() > 0 {
        notes.push(s::includes_installments(&format::money(d.installments)));
    }
    if open {
        notes.push(s::OPEN_NOTE.to_owned());
    }
    let note = (!notes.is_empty()).then(|| notes.join(" "));
    (subtitle, d.status, rows, note)
}

fn loan_rows(d: &LoanDue) -> (String, Status, Rows, Option<String>) {
    let p = &d.payment;
    let rows: Rows = vec![
        (s::DUE_DATE, format::date(p.date), true),
        (s::PAYMENT, format::money(p.amount), true),
        (s::PRINCIPAL, format::money(p.principal), false),
        (s::INTEREST, format::money(p.interest), false),
        (s::BALANCE_AFTER, format::money(p.balance_after), false),
        (s::PAID, format::money(d.paid), false),
    ];
    (s::payment_n_of(d.number, d.of), d.status, rows, None)
}
