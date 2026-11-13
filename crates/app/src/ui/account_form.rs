//! Adding and editing an account, including a card's or loan's settings.

use billo_api::rules::{DayCount, DueShift, InterestStart, LoanMethod, RoundingMode};
use billo_api::{AccountData, AccountKind, Record};
use dioxus::prelude::*;

use super::accounts::BackLink;
use super::fields::{CheckField, NumberField, SelectField, TextField};
use super::{ErrorBox, Problem, Screen, use_nav};
use crate::forms::{AccountDraft, CURRENCIES, DueMode, from_key, is_card_like, key};
use crate::store::use_store;
use crate::{api, clock, strings as s};

const KINDS: [AccountKind; 5] = [
    AccountKind::Card,
    AccountKind::Revolving,
    AccountKind::Loan,
    AccountKind::Bank,
    AccountKind::Cash,
];

/// Step 1 of adding an account: what kind is it?
#[component]
pub fn PickKind() -> Element {
    let mut nav = use_nav();
    rsx! {
        BackLink { to: Screen::Accounts }
        h2 { "{s::add_account()}" }
        section { class: "card",
            ul { class: "list",
                for k in KINDS {
                    li { class: "clickable",
                        button {
                            class: "row-button",
                            id: "kind-{key(&k)}",
                            onclick: move |_| nav.set(Screen::NewAccount(Some(k))),
                            span { class: "main",
                                span { class: "title", "{s::kind_label(k)}" }
                                span { class: "sub", "{s::kind_hint(k)}" }
                            }
                            span { class: "sub", "aria-hidden": "true", "›" }
                        }
                    }
                }
            }
        }
    }
}

fn options<T: serde::Serialize>(items: &[(T, &'static str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(v, l)| (key(v), (*l).to_owned()))
        .collect()
}

fn rounding_steps() -> Vec<(String, String)> {
    ["0.01", "1", "10"]
        .iter()
        .map(|v| ((*v).to_owned(), (*v).to_owned()))
        .collect()
}

fn rounding_modes() -> Vec<(String, String)> {
    options(&[
        (RoundingMode::HalfUp, s::round_nearest()),
        (RoundingMode::Up, s::round_up()),
        (RoundingMode::Down, s::round_down()),
    ])
}

fn day_counts() -> Vec<(String, String)> {
    options(&[
        (DayCount::Actual365, s::day_count_label(DayCount::Actual365)),
        (
            DayCount::ActualActual,
            s::day_count_label(DayCount::ActualActual),
        ),
        (DayCount::Actual360, s::day_count_label(DayCount::Actual360)),
    ])
}

/// New account of `kind`, or edit `existing`.
#[component]
pub fn AccountForm(kind: AccountKind, existing: Option<Record<AccountData>>) -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let today = clock::today();
    let editing = existing.clone();
    let mut draft = use_signal(move || match &editing {
        Some(r) => AccountDraft::from_data(&r.data, today),
        None => AccountDraft::new(kind, today),
    });
    let mut error = use_signal(|| None::<Problem>);
    let mut busy = use_signal(|| false);

    let new_id = use_hook(api::new_id);
    let id = existing.as_ref().map_or(new_id, |r| r.id);
    let back = match &existing {
        Some(r) => Screen::Account(r.id),
        None => Screen::NewAccount(None),
    };

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let data = match draft.read().to_data() {
            Ok(d) => d,
            Err(msg) => {
                error.set(Some(msg.into()));
                return;
            }
        };
        error.set(None);
        busy.set(true);
        spawn(async move {
            match store
                .call(|c| async move { c.put_account(id, data).await })
                .await
            {
                Ok(saved) => nav.set(Screen::Account(saved.id)),
                Err(e) => error.set(Some(e.into())),
            }
            busy.set(false);
        });
    };

    let d = draft.read().clone();
    let title = if existing.is_some() {
        s::edit_account()
    } else {
        s::new_account_title(kind)
    };
    let currencies: Vec<(String, String)> = CURRENCIES
        .iter()
        .map(|c| ((*c).to_owned(), (*c).to_owned()))
        .collect();

    rsx! {
        BackLink { to: back }
        h2 { "{title}" }
        form { class: "card", id: "account-form", onsubmit: submit,
            TextField {
                id: "name",
                label: s::name(),
                value: d.name.clone(),
                placeholder: s::name_placeholder(d.kind),
                oninput: move |v| draft.write().name = v,
            }
            SelectField {
                id: "currency",
                label: s::currency(),
                value: d.currency.clone(),
                options: currencies,
                onchange: move |v| draft.write().currency = v,
            }
            if is_card_like(d.kind) {
                CardFields { draft }
            }
            if d.kind == AccountKind::Loan {
                LoanFields { draft }
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg.message, details: msg.details }
            }
            button { r#type: "submit", id: "save-account", disabled: busy(), "{s::save()}" }
        }
    }
}

#[component]
fn CardFields(draft: Signal<AccountDraft>) -> Element {
    let d = draft.read().clone();
    let due_mode = if d.due_mode == DueMode::DaysAfter {
        "days"
    } else {
        "day"
    };
    let (due_label, due_hint) = match d.due_mode {
        DueMode::DaysAfter => (s::days_after_statement(), s::days_after_hint()),
        DueMode::DayOfMonth => (s::due_day_of_month(), s::due_day_hint()),
    };
    rsx! {
        p { class: "hint example", "{s::example_values()}" }
        fieldset {
            legend { "{s::statement_and_due()}" }
            NumberField {
                id: "statement-day",
                label: s::statement_day(),
                value: d.statement_day.clone(),
                hint: s::statement_day_hint().to_owned(),
                oninput: move |v| draft.write().statement_day = v,
            }
            div { class: "pair",
                SelectField {
                    id: "due-mode",
                    label: s::due_rule(),
                    value: due_mode.to_owned(),
                    options: vec![
                        ("days".to_owned(), s::due_mode_days().to_owned()),
                        ("day".to_owned(), s::due_mode_day().to_owned()),
                    ],
                    onchange: move |v: String| {
                        draft.write().due_mode = if v == "day" { DueMode::DayOfMonth } else { DueMode::DaysAfter };
                    },
                }
                NumberField {
                    id: "due-value",
                    label: due_label,
                    value: d.due_value.clone(),
                    oninput: move |v| draft.write().due_value = v,
                }
            }
            p { class: "hint", "{due_hint}" }
            SelectField {
                id: "due-shift",
                label: s::if_not_business_day(),
                value: key(&d.due_shift),
                options: options(&[
                    (DueShift::Next, s::due_shift_label(DueShift::Next)),
                    (DueShift::Previous, s::due_shift_label(DueShift::Previous)),
                    (DueShift::None, s::due_shift_label(DueShift::None)),
                ]),
                onchange: move |v: String| {
                    if let Some(x) = from_key(&v) { draft.write().due_shift = x; }
                },
            }
        }
        fieldset {
            legend { "{s::minimum_payment()}" }
            div { class: "pair",
                NumberField {
                    id: "min-percent",
                    label: s::min_percent(),
                    value: d.min_percent.clone(),
                    oninput: move |v| draft.write().min_percent = v,
                }
                NumberField {
                    id: "min-floor",
                    label: s::min_floor(),
                    value: d.min_floor.clone(),
                    oninput: move |v| draft.write().min_floor = v,
                }
            }
            p { class: "hint", "{s::min_hint()}" }
            div { class: "pair",
                SelectField {
                    id: "min-round-step",
                    label: s::round_to(),
                    value: d.min_round_step.clone(),
                    options: rounding_steps(),
                    onchange: move |v| draft.write().min_round_step = v,
                }
                SelectField {
                    id: "min-round-mode",
                    label: s::rounding(),
                    value: key(&d.min_round_mode),
                    options: rounding_modes(),
                    onchange: move |v: String| {
                        if let Some(x) = from_key(&v) { draft.write().min_round_mode = x; }
                    },
                }
            }
        }
        fieldset {
            legend { "{s::interest()}" }
            NumberField {
                id: "rate",
                label: s::interest_rate_year(),
                value: d.rate.clone(),
                oninput: move |v| draft.write().rate = v,
            }
            div { class: "pair",
                SelectField {
                    id: "day-count",
                    label: s::day_count(),
                    value: key(&d.day_count),
                    options: day_counts(),
                    onchange: move |v: String| {
                        if let Some(x) = from_key(&v) { draft.write().day_count = x; }
                    },
                }
                SelectField {
                    id: "interest-start",
                    label: s::interest_from(),
                    value: key(&d.interest_start),
                    options: options(&[
                        (InterestStart::TransactionDate, s::interest_start_label(InterestStart::TransactionDate)),
                        (InterestStart::PostingDate, s::interest_start_label(InterestStart::PostingDate)),
                    ]),
                    onchange: move |v: String| {
                        if let Some(x) = from_key(&v) { draft.write().interest_start = x; }
                    },
                }
            }
            CheckField {
                id: "grace",
                label: s::grace_label(),
                checked: d.grace,
                onchange: move |v| draft.write().grace = v,
            }
            div { class: "pair",
                SelectField {
                    id: "interest-round-step",
                    label: s::round_to(),
                    value: d.interest_round_step.clone(),
                    options: rounding_steps(),
                    onchange: move |v| draft.write().interest_round_step = v,
                }
                SelectField {
                    id: "interest-round-mode",
                    label: s::rounding(),
                    value: key(&d.interest_round_mode),
                    options: rounding_modes(),
                    onchange: move |v: String| {
                        if let Some(x) = from_key(&v) { draft.write().interest_round_mode = x; }
                    },
                }
            }
        }
        fieldset {
            legend { "{s::credit_limit()}" }
            NumberField {
                id: "credit-limit",
                label: s::credit_limit_optional(),
                value: d.credit_limit.clone(),
                oninput: move |v| draft.write().credit_limit = v,
            }
        }
    }
}

#[component]
fn LoanFields(draft: Signal<AccountDraft>) -> Element {
    let d = draft.read().clone();
    rsx! {
        fieldset {
            legend { "{s::loan_terms()}" }
            NumberField {
                id: "principal",
                label: s::amount_borrowed(),
                value: d.principal.clone(),
                oninput: move |v| draft.write().principal = v,
            }
            div { class: "pair",
                NumberField {
                    id: "loan-rate",
                    label: s::interest_rate_year(),
                    value: d.loan_rate.clone(),
                    oninput: move |v| draft.write().loan_rate = v,
                }
                NumberField {
                    id: "months",
                    label: s::months(),
                    value: d.months.clone(),
                    oninput: move |v| draft.write().months = v,
                }
            }
            div { class: "pair",
                TextField {
                    id: "start",
                    label: s::start_date(),
                    input_type: "date",
                    value: d.start.clone(),
                    oninput: move |v| draft.write().start = v,
                }
                TextField {
                    id: "first-payment",
                    label: s::first_payment(),
                    input_type: "date",
                    value: d.first_payment.clone(),
                    oninput: move |v| draft.write().first_payment = v,
                }
            }
            SelectField {
                id: "method",
                label: s::method(),
                value: key(&d.method),
                options: options(&[
                    (LoanMethod::Effective, s::loan_method_label(LoanMethod::Effective)),
                    (LoanMethod::Flat, s::loan_method_label(LoanMethod::Flat)),
                ]),
                onchange: move |v: String| {
                    if let Some(x) = from_key(&v) { draft.write().method = x; }
                },
            }
            if d.method == LoanMethod::Effective {
                SelectField {
                    id: "loan-day-count",
                    label: s::day_count(),
                    value: key(&d.loan_day_count),
                    options: day_counts(),
                    onchange: move |v: String| {
                        if let Some(x) = from_key(&v) { draft.write().loan_day_count = x; }
                    },
                }
            }
            NumberField {
                id: "installment",
                label: s::stated_installment_optional(),
                value: d.installment.clone(),
                hint: s::stated_installment_hint().to_owned(),
                oninput: move |v| draft.write().installment = v,
            }
        }
    }
}
