//! "This month": every card statement and loan payment, sorted by due date.

use billo_api::TransactionQuery;
use dioxus::prelude::*;

use super::categories::StarterOffer;
use super::{ErrorBox, Loading, Screen, use_holidays, use_nav};
use crate::api::ApiError;
use crate::overview::{self, CardDue, Detail, Item, LoanDue, Status, ThisMonth};
use crate::store::use_store;
use crate::{clock, format, strings as s};

#[component]
pub fn Home() -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let holidays = use_holidays();
    let mut data = use_resource(move || async move {
        // Read before awaiting, so a change in More re-runs this.
        let cal = holidays.calendar();
        let (accounts, txns, plans) = store
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
            overview::this_month(&accounts, &txns, &plans, today, &cal),
        ))
    });

    let body = match &*data.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), details: e.details(), on_retry: move |_| data.restart() }
        },
        Some(Ok((_, true, _))) => rsx! {
            div { class: "empty",
                p { "{s::home_empty()}" }
                button { onclick: move |_| nav.set(Screen::NewAccount(None)), "{s::add_account()}" }
            }
        },
        Some(Ok((today, false, month))) => {
            let ThisMonth { items, errors } = month.clone();
            let nothing = items.is_empty() && errors.is_empty();
            rsx! {
                p { class: "note today", "{s::today()} {format::date(*today)}" }
                for e in errors {
                    section { class: "card item-error", key: "err-{e.account_id}",
                        div { class: "card-head",
                            h3 { "{e.account_name}" }
                        }
                        ErrorBox { message: format!("{}: {}", s::cant_work_out(), e.message) }
                    }
                }
                for item in items {
                    ItemCard { item }
                }
                if nothing {
                    p { class: "empty", "{s::nothing_due()}" }
                }
            }
        }
    };

    // New users with no categories get the starter set offered once.
    let mut cats =
        use_resource(
            move || async move { store.call(|c| async move { c.categories().await }).await },
        );
    let mut dismissed = use_signal(|| false);
    let offer = !dismissed() && matches!(&*cats.read(), Some(Ok(list)) if list.is_empty());

    rsx! {
        h2 { "{s::this_month()}" }
        if offer {
            StarterOffer {
                on_added: move |()| cats.restart(),
                on_dismiss: move |()| dismissed.set(true),
            }
        }
        {body}
    }
}

fn status_badge(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::NothingDue => ("badge", s::status_nothing_due()),
        Status::PaidInFull => ("badge paid", s::status_paid_in_full()),
        Status::Paid => ("badge paid", s::status_paid()),
        Status::MinimumPaid => ("badge paid", s::status_minimum_paid()),
        Status::Unpaid => ("badge unpaid", s::status_unpaid()),
        Status::Overdue => ("badge overdue", s::status_overdue()),
        Status::Open => ("badge", s::status_open()),
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
            s::next_statement(),
            format::date(st.period.statement_date)
        )
    } else {
        format!(
            "{} {}",
            s::statement_of(),
            format::date(st.period.statement_date)
        )
    };
    let mut rows: Rows = vec![
        (s::due_date(), format::date(st.period.due_date), true),
        (
            s::opening_balance(),
            format::money(st.opening_balance),
            false,
        ),
        (s::new_charges(), format::money(st.charges), false),
        (s::payments_and_credits(), format::money(st.credits), false),
        (s::interest(), format::money(st.interest), false),
    ];
    if open {
        rows.push((s::balance_so_far(), format::money(st.closing_balance), true));
        rows.push((s::minimum_so_far(), format::money(st.minimum_due), true));
    } else {
        rows.push((
            s::closing_balance(),
            format::money(st.closing_balance),
            true,
        ));
        rows.push((s::minimum_due(), format::money(st.minimum_due), true));
        rows.push((s::paid_since_statement(), format::money(d.paid), false));
    }
    if st.overdue.minor() > 0 {
        rows.push((s::overdue_from_last(), format::money(st.overdue), false));
    }
    let mut notes = Vec::new();
    if d.installments.minor() > 0 {
        notes.push(s::includes_installments(&format::money(d.installments)));
    }
    if open {
        notes.push(s::open_note().to_owned());
    }
    let note = (!notes.is_empty()).then(|| notes.join(" "));
    (subtitle, d.status, rows, note)
}

fn loan_rows(d: &LoanDue) -> (String, Status, Rows, Option<String>) {
    let p = &d.payment;
    let rows: Rows = vec![
        (s::due_date(), format::date(p.date), true),
        (s::payment(), format::money(p.amount), true),
        (s::principal(), format::money(p.principal), false),
        (s::interest(), format::money(p.interest), false),
        (s::balance_after(), format::money(p.balance_after), false),
        (s::paid(), format::money(d.paid), false),
    ];
    (s::payment_n_of(d.number, d.of), d.status, rows, None)
}
