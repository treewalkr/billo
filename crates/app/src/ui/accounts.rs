//! Accounts: the list (grouped by kind) and one account's page.

use billo_api::rules::DueRule;
use billo_api::{AccountData, AccountKind, Record};
use billo_domain::{Currency, Money};
use dioxus::prelude::*;
use uuid::Uuid;

use super::plans::PlanList;
use super::transactions::TransactionList;
use super::{ErrorBox, Loading, Screen, use_auth, use_nav};
use crate::forms::is_card_like;
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
            s::MONTHS_SHORT
        );
    }
    d.currency.clone()
}

#[component]
pub fn Accounts() -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let mut accounts =
        use_resource(move || async move { auth.call(|c| async move { c.accounts().await }).await });

    let body = match &*accounts.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), on_retry: move |_| accounts.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "empty", "{s::NO_ACCOUNTS}" }
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
                                li {
                                    key: "{a.id}",
                                    class: "clickable account",
                                    onclick: move |_| nav.set(Screen::Account(a.id)),
                                    div { class: "main",
                                        span { class: "title", "{a.data.name}" }
                                        span { class: "sub", "{summary(&a.data)}" }
                                    }
                                    span { class: "sub", "›" }
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
            h2 { "{s::ACCOUNTS}" }
            button { id: "add-account", onclick: move |_| nav.set(Screen::NewAccount(None)), "{s::ADD_ACCOUNT}" }
        }
        {body}
    }
}

/// One account: its settings, with edit and delete.
#[component]
pub fn AccountPage(id: Uuid) -> Element {
    let auth = use_auth();
    let mut account =
        use_resource(
            move || async move { auth.call(|c| async move { c.account(id).await }).await },
        );

    let record = match &*account.read() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => {
            return rsx! {
                BackLink { to: Screen::Accounts }
                ErrorBox { message: e.to_string(), on_retry: move |_| account.restart() }
            };
        }
        Some(Ok(r)) => r.clone(),
    };

    rsx! {
        BackLink { to: Screen::Accounts }
        section { class: "card", id: "account-detail",
            div { class: "card-head",
                h3 { "{record.data.name}" }
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
    let mut rows: Vec<(&'static str, String)> = vec![(s::CURRENCY, data.currency.clone())];
    if let Some(c) = &data.rules.card {
        let due = match c.due {
            DueRule::DaysAfterStatement(n) => s::due_days_after(n),
            DueRule::DayOfMonth(n) => s::due_on_day(n),
        };
        rows.extend([
            (s::STATEMENT_DAY, c.statement_day.to_string()),
            (s::DUE_DATE, due),
            (
                s::IF_NOT_BUSINESS_DAY,
                s::due_shift_label(c.due_shift).to_owned(),
            ),
            (
                s::MINIMUM_PAYMENT,
                format!(
                    "{}% {} {}",
                    format::percent(c.min_payment.percent_bp),
                    s::OR_AT_LEAST,
                    amount(c.min_payment.floor_minor, &data.currency)
                ),
            ),
            (
                s::INTEREST_RATE,
                format!(
                    "{}% {}",
                    format::percent(c.interest.annual_rate_bp),
                    s::PER_YEAR
                ),
            ),
            (
                s::DAY_COUNT,
                s::day_count_label(c.interest.day_count).to_owned(),
            ),
            (
                s::INTEREST_FROM,
                s::interest_start_label(c.interest.start).to_owned(),
            ),
            (
                s::GRACE_PERIOD,
                if c.interest.grace_if_paid_in_full {
                    s::YES
                } else {
                    s::NO
                }
                .to_owned(),
            ),
        ]);
        if let Some(l) = c.credit_limit_minor {
            rows.push((s::CREDIT_LIMIT, amount(l, &data.currency)));
        }
    }
    if let Some(l) = &data.rules.loan {
        rows.extend([
            (
                s::AMOUNT_BORROWED,
                amount(l.principal_minor, &data.currency),
            ),
            (
                s::INTEREST_RATE,
                format!("{}% {}", format::percent(l.annual_rate_bp), s::PER_YEAR),
            ),
            (s::METHOD, s::loan_method_label(l.method).to_owned()),
            (s::MONTHS, l.months.to_string()),
            (s::START_DATE, format::date(l.start)),
            (s::FIRST_PAYMENT, format::date(l.first_payment)),
        ]);
        if let Some(i) = l.installment_minor {
            rows.push((s::STATED_INSTALLMENT, amount(i, &data.currency)));
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
    let auth = use_auth();
    let mut nav = use_nav();
    let mut confirm = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let id = record.id;
    let delete = move |_| {
        if !confirm() {
            confirm.set(true);
            return;
        }
        spawn(async move {
            match auth
                .call(|c| async move { c.delete_account(id).await })
                .await
            {
                Ok(()) => nav.set(Screen::Accounts),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    rsx! {
        if let Some(msg) = error() {
            ErrorBox { message: msg }
        }
        div { class: "actions",
            button {
                class: "secondary",
                id: "edit-account",
                onclick: move |_| nav.set(Screen::EditAccount(Box::new(record.clone()))),
                "{s::EDIT}"
            }
            button { class: "danger", id: "delete-account", onclick: delete,
                if confirm() { "{s::CONFIRM_DELETE}" } else { "{s::DELETE}" }
            }
        }
    }
}

#[component]
pub fn BackLink(to: Screen) -> Element {
    let mut nav = use_nav();
    rsx! {
        button { class: "link back", onclick: move |_| nav.set(to.clone()), "‹ {s::BACK}" }
    }
}
