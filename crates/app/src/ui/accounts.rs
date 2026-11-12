//! Accounts: the list (grouped by kind) and one account's page.

use billo_api::rules::DueRule;
use billo_api::{AccountData, AccountKind, Record};
use billo_domain::{Currency, Money};
use dioxus::prelude::*;
use uuid::Uuid;

use super::plans::PlanList;
use super::transactions::TransactionList;
use super::{ErrorBox, Loading, Problem, Screen, use_nav};
use crate::forms::is_card_like;
use crate::store::use_store;
use crate::{format, strings as s};

/// List order of the groups.
const GROUPS: [AccountKind; 5] = [
    AccountKind::Card,
    AccountKind::Revolving,
    AccountKind::Loan,
    AccountKind::Bank,
    AccountKind::Cash,
];

fn amount(minor: i64, currency: &str) -> String {
    match Currency::from_code(currency) {
        Some(c) => format::money(Money::new(minor, c)),
        None => format!("{minor} {currency}"),
    }
}

/// One line describing the account's settings.
pub fn summary(d: &AccountData) -> String {
    if let Some(c) = &d.rules.card {
        let due = match c.due {
            DueRule::DaysAfterStatement(n) => s::due_days_after(n),
            DueRule::DayOfMonth(n) => s::due_on_day(n),
        };
        return format!("{} · {due}", s::statement_on_day(c.statement_day));
    }
    if let Some(l) = &d.rules.loan {
        return format!(
            "{} · {}% · {} {}",
            amount(l.principal_minor, &d.currency),
            format::percent(l.annual_rate_bp),
            l.months,
            s::months_short()
        );
    }
    d.currency.clone()
}

#[component]
pub fn Accounts() -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let mut accounts =
        use_resource(
            move || async move { store.call(|c| async move { c.accounts().await }).await },
        );

    let body = match &*accounts.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), details: e.details(), on_retry: move |_| accounts.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "empty", "{s::no_accounts()}" }
        },
        Some(Ok(list)) => {
            let groups: Vec<(AccountKind, Vec<Record<AccountData>>)> = GROUPS
                .iter()
                .map(|k| {
                    let items: Vec<_> =
                        list.iter().filter(|a| a.data.kind == *k).cloned().collect();
                    (*k, items)
                })
                .filter(|(_, items)| !items.is_empty())
                .collect();
            rsx! {
                for (kind, items) in groups {
                    h3 { class: "group-title", "{s::kind_group(kind)}" }
                    section { class: "card",
                        ul { class: "list",
                            for a in items {
                                li { key: "{a.id}", class: "clickable account",
                                    button {
                                        class: "row-button",
                                        onclick: move |_| nav.set(Screen::Account(a.id)),
                                        span { class: "main",
                                            span { class: "title", "{a.data.name}" }
                                            span { class: "sub", "{summary(&a.data)}" }
                                        }
                                        span { class: "sub", "aria-hidden": "true", "›" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        div { class: "toolbar",
            h2 { "{s::accounts()}" }
            button { id: "add-account", onclick: move |_| nav.set(Screen::NewAccount(None)), "{s::add_account()}" }
        }
        {body}
    }
}

/// One account: its settings, with edit and delete.
#[component]
pub fn AccountPage(id: Uuid) -> Element {
    let store = use_store();
    let mut account =
        use_resource(
            move || async move { store.call(|c| async move { c.account(id).await }).await },
        );

    let record = match &*account.read() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => {
            return rsx! {
                BackLink { to: Screen::Accounts }
                ErrorBox { message: e.to_string(), details: e.details(), on_retry: move |_| account.restart() }
            };
        }
        Some(Ok(r)) => r.clone(),
    };

    rsx! {
        BackLink { to: Screen::Accounts }
        section { class: "card", id: "account-detail",
            div { class: "card-head",
                h2 { "{record.data.name}" }
                span { class: "badge", "{s::kind_label(record.data.kind)}" }
            }
            Settings { data: record.data.clone() }
            AccountActions { record: record.clone() }
        }
        if is_card_like(record.data.kind) {
            PlanList { account: record.clone() }
        }
        TransactionList { account: record.clone() }
    }
}

#[component]
fn Settings(data: AccountData) -> Element {
    let mut rows: Vec<(&'static str, String)> = vec![(s::currency(), data.currency.clone())];
    if let Some(c) = &data.rules.card {
        let due = match c.due {
            DueRule::DaysAfterStatement(n) => s::due_days_after(n),
            DueRule::DayOfMonth(n) => s::due_on_day(n),
        };
        rows.extend([
            (s::statement_day(), c.statement_day.to_string()),
            (s::due_date(), due),
            (
                s::if_not_business_day(),
                s::due_shift_label(c.due_shift).to_owned(),
            ),
            (
                s::minimum_payment(),
                format!(
                    "{}% {} {}",
                    format::percent(c.min_payment.percent_bp),
                    s::or_at_least(),
                    amount(c.min_payment.floor_minor, &data.currency)
                ),
            ),
            (
                s::interest_rate(),
                format!(
                    "{}% {}",
                    format::percent(c.interest.annual_rate_bp),
                    s::per_year()
                ),
            ),
            (
                s::day_count(),
                s::day_count_label(c.interest.day_count).to_owned(),
            ),
            (
                s::interest_from(),
                s::interest_start_label(c.interest.start).to_owned(),
            ),
            (
                s::grace_period(),
                if c.interest.grace_if_paid_in_full {
                    s::yes()
                } else {
                    s::no()
                }
                .to_owned(),
            ),
        ]);
        if let Some(l) = c.credit_limit_minor {
            rows.push((s::credit_limit(), amount(l, &data.currency)));
        }
    }
    if let Some(l) = &data.rules.loan {
        rows.extend([
            (
                s::amount_borrowed(),
                amount(l.principal_minor, &data.currency),
            ),
            (
                s::interest_rate(),
                format!("{}% {}", format::percent(l.annual_rate_bp), s::per_year()),
            ),
            (s::method(), s::loan_method_label(l.method).to_owned()),
            (s::months(), l.months.to_string()),
            (s::start_date(), format::date(l.start)),
            (s::first_payment(), format::date(l.first_payment)),
        ]);
        if let Some(i) = l.installment_minor {
            rows.push((s::stated_installment(), amount(i, &data.currency)));
        }
    }
    rsx! {
        dl {
            for (label, value) in rows {
                div { class: "row",
                    dt { "{label}" }
                    dd { "{value}" }
                }
            }
        }
    }
}

#[component]
fn AccountActions(record: Record<AccountData>) -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let mut confirm = use_signal(|| false);
    let mut error = use_signal(|| None::<Problem>);
    let id = record.id;
    let delete = move |_| {
        if !confirm() {
            confirm.set(true);
            return;
        }
        spawn(async move {
            match store
                .call(|c| async move { c.delete_account(id).await })
                .await
            {
                Ok(()) => nav.set(Screen::Accounts),
                Err(e) => error.set(Some(e.into())),
            }
        });
    };
    rsx! {
        if let Some(msg) = error() {
            ErrorBox { message: msg.message, details: msg.details }
        }
        div { class: "actions",
            button {
                class: "secondary",
                id: "edit-account",
                onclick: move |_| nav.set(Screen::EditAccount(Box::new(record.clone()))),
                "{s::edit()}"
            }
            button { class: "danger", id: "delete-account", onclick: delete,
                if confirm() { "{s::confirm_delete()}" } else { "{s::delete()}" }
            }
        }
    }
}

#[component]
pub fn BackLink(to: Screen) -> Element {
    let mut nav = use_nav();
    rsx! {
        button { class: "link back", onclick: move |_| nav.set(to.clone()), "‹ {s::back()}" }
    }
}
