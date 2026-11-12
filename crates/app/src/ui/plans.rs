//! Installment plans (ผ่อนชำระ) on a card.

use billo_api::{AccountData, InstallmentPlanData, Record};
use dioxus::prelude::*;

use super::accounts::BackLink;
use super::fields::{NumberField, TextField};
use super::{ErrorBox, Loading, Screen, use_nav};
use crate::forms::PlanDraft;
use crate::store::use_store;
use crate::{api, clock, convert, format, strings as s};

/// One line about a plan, or why it can't be read.
fn describe(p: &InstallmentPlanData) -> (String, String) {
    match (convert::installment_plan(p), convert::money(&p.principal)) {
        (Ok(plan), Ok(principal)) => {
            let portion = plan
                .schedule()
                .first()
                .map(|x| format::money(x.amount()))
                .unwrap_or_default();
            (
                format!(
                    "{} · {} {}",
                    format::money(principal),
                    p.months,
                    s::months_short()
                ),
                format!(
                    "{portion}{} · {}% {} · {} {}",
                    s::per_month(),
                    format::percent(p.monthly_rate_bp),
                    s::per_month_rate(),
                    s::first_posting_short(),
                    format::date(p.first_post_on)
                ),
            )
        }
        (Err(e), _) | (_, Err(e)) => (s::invalid_plan().to_owned(), e.to_string()),
    }
}

#[component]
pub fn PlanList(account: Record<AccountData>) -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let card_id = account.id;
    let mut plans = use_resource(move || async move {
        let all = store
            .call(|c| async move { c.installment_plans().await })
            .await?;
        Ok::<_, api::ApiError>(
            all.into_iter()
                .filter(|p| p.data.card_account_id == card_id)
                .collect::<Vec<_>>(),
        )
    });
    let add_account = account.clone();
    let body = match &*plans.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), on_retry: move |_| plans.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! { p { class: "empty", "{s::no_plans()}" } },
        Some(Ok(list)) => rsx! {
            ul { class: "list", id: "plans",
                for p in list.clone() {
                    li {
                        key: "{p.id}",
                        class: "clickable plan",
                        onclick: {
                            let account = account.clone();
                            let p = p.clone();
                            move |_| nav.set(Screen::PlanForm(Box::new(account.clone()), Some(Box::new(p.clone()))))
                        },
                        div { class: "main",
                            span { class: "title", "{describe(&p.data).0}" }
                            span { class: "sub", "{describe(&p.data).1}" }
                        }
                        span { class: "sub", "›" }
                    }
                }
            }
        },
    };
    rsx! {
        section { class: "card",
            div { class: "card-head",
                h3 { "{s::installment_plans()}" }
                button {
                    class: "secondary",
                    id: "add-plan",
                    onclick: move |_| nav.set(Screen::PlanForm(Box::new(add_account.clone()), None)),
                    "{s::add()}"
                }
            }
            {body}
        }
    }
}

#[component]
pub fn PlanForm(
    account: Record<AccountData>,
    existing: Option<Record<InstallmentPlanData>>,
) -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let today = clock::today();
    let init = existing.clone();
    let mut draft = use_signal(move || match &init {
        Some(p) => PlanDraft::from_data(&p.data),
        None => PlanDraft::new(today),
    });
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut confirm = use_signal(|| false);
    let new_id = use_hook(api::new_id);
    let id = existing.as_ref().map_or(new_id, |p| p.id);
    let card_id = account.id;
    let currency = account.data.currency.clone();

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let data = match draft.read().to_data(card_id, &currency) {
            Ok(d) => d,
            Err(msg) => {
                error.set(Some(msg));
                return;
            }
        };
        error.set(None);
        busy.set(true);
        spawn(async move {
            match store
                .call(|c| async move { c.put_installment_plan(id, data).await })
                .await
            {
                Ok(_) => nav.set(Screen::Account(card_id)),
                Err(e) => error.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };
    let delete = move |_| {
        if !confirm() {
            confirm.set(true);
            return;
        }
        spawn(async move {
            match store
                .call(|c| async move { c.delete_installment_plan(id).await })
                .await
            {
                Ok(()) => nav.set(Screen::Account(card_id)),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };

    let d = draft.read().clone();
    let title = if existing.is_some() {
        s::edit_plan()
    } else {
        s::add_plan()
    };
    rsx! {
        BackLink { to: Screen::Account(card_id) }
        h2 { "{title}" }
        p { class: "note", "{account.data.name}" }
        form { class: "card", id: "plan-form", onsubmit: submit,
            NumberField {
                id: "plan-principal",
                label: s::plan_amount(),
                value: d.principal.clone(),
                placeholder: "0.00",
                oninput: move |v| draft.write().principal = v,
            }
            div { class: "pair",
                NumberField {
                    id: "plan-months",
                    label: s::months(),
                    value: d.months.clone(),
                    oninput: move |v| draft.write().months = v,
                }
                NumberField {
                    id: "plan-rate",
                    label: s::monthly_rate(),
                    value: d.monthly_rate.clone(),
                    hint: s::monthly_rate_hint().to_owned(),
                    oninput: move |v| draft.write().monthly_rate = v,
                }
            }
            TextField {
                id: "plan-first",
                label: s::first_posting(),
                input_type: "date",
                value: d.first_post_on.clone(),
                hint: s::first_posting_hint().to_owned(),
                oninput: move |v| draft.write().first_post_on = v,
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg }
            }
            button { r#type: "submit", id: "save-plan", disabled: busy(), "{s::save()}" }
            if existing.is_some() {
                div { class: "actions",
                    button { r#type: "button", class: "danger", id: "delete-plan", onclick: delete,
                        if confirm() { "{s::confirm_delete()}" } else { "{s::delete()}" }
                    }
                }
            }
        }
    }
}
