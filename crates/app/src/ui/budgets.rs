//! Budgets: a limit per spending category for a month, against what was spent.

use billo_api::{BudgetData, BudgetQuery, CategoryKind, Record, TransactionQuery};
use billo_domain::Money;
use billo_domain::budget::CategoryBudget;
use dioxus::prelude::*;
use uuid::Uuid;

use super::fields::NumberField;
use super::{ErrorBox, Loading, Problem, Screen, use_nav};
use crate::budgets::{self, CURRENCY, Key};
use crate::input::{field, parse_amount};
use crate::store::use_store;
use crate::{api, categories, clock, format, strings as s};

/// One spending category on the screen.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: Uuid,
    parent: Option<Uuid>,
    /// The category's own name.
    name: String,
    /// What the row shows: the name, or `Parent › Name` when the parent is elsewhere.
    title: String,
    depth: u8,
    budget: Option<Record<BudgetData>>,
    figures: Option<CategoryBudget<Key>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Summary {
    limit: Money,
    spent: Money,
    remaining: Money,
    unbudgeted: Money,
}

#[component]
pub fn Budgets() -> Element {
    let store = use_store();
    let mut nav = use_nav();
    let today = clock::today();
    let mut shown = use_signal(|| (today.year(), today.month()));
    let mut data = use_resource(move || async move {
        let (year, month) = shown();
        let (first, last) =
            budgets::bounds(year, month).ok_or_else(|| s::cant_work_out_budget().to_owned())?;
        let (accounts, cats, txns, list) = store
            .call(|c| async move {
                let accounts = c.accounts().await?;
                let cats = c.categories().await?;
                let q = TransactionQuery {
                    account_id: None,
                    from: Some(first),
                    to: Some(last),
                };
                let txns = c.transactions(&q).await?;
                let list = c
                    .budgets(&BudgetQuery {
                        period_month: Some(first),
                    })
                    .await?;
                Ok((accounts, cats, txns, list))
            })
            .await
            .map_err(|e| e.to_string())?;
        let result = budgets::month(year, month, &accounts, &cats, &txns, &list)
            .map_err(|e| format!("{}: {e}", s::cant_work_out_budget()))?;
        let rows: Vec<Row> = categories::tree(&cats, CategoryKind::Expense)
            .into_iter()
            .map(|(c, depth)| Row {
                id: c.id,
                parent: c.data.parent_id.filter(|_| depth > 0),
                name: categories::name(&c.data),
                title: categories::label(&cats, &c.data),
                depth,
                budget: list
                    .iter()
                    .find(|b| b.data.category_id == c.id && b.data.period_month == first)
                    .cloned(),
                figures: result
                    .categories
                    .iter()
                    .find(|x| x.category == Some(c.id))
                    .cloned(),
            })
            .collect();
        let summary = Summary {
            limit: result.total_limit,
            spent: result.total_spent,
            remaining: result.total_remaining,
            unbudgeted: result.unbudgeted_spent,
        };
        Ok::<_, String>((first, rows, summary))
    });

    let (year, month) = shown();
    let body = match &*data.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.clone(), on_retry: move |_| data.restart() }
        },
        Some(Ok((_, rows, _))) if rows.is_empty() => rsx! {
            div { class: "empty",
                p { "{s::budgets_need_categories()}" }
                button { onclick: move |_| nav.set(Screen::Categories), "{s::categories()}" }
            }
        },
        Some(Ok((first, rows, sum))) => {
            let budgeted = group(rows, true);
            let others = group(rows, false);
            let first = *first;
            let over = sum.remaining.is_negative();
            rsx! {
                section { class: "card", id: "budget-summary",
                    dl {
                        div { class: "row", "data-label": "budgeted",
                            dt { "{s::budgeted()}" }
                            dd { "{format::money(sum.limit)}" }
                        }
                        div { class: "row", "data-label": "spent",
                            dt { "{s::spent()}" }
                            dd { "{format::money(sum.spent)}" }
                        }
                        div { class: if over { "row strong over" } else { "row strong" }, "data-label": "left",
                            dt { "{s::left()}" }
                            dd { "{format::money(sum.remaining)}" }
                        }
                        div { class: "row", "data-label": "unbudgeted",
                            dt { "{s::unbudgeted()}" }
                            dd { "{format::money(sum.unbudgeted)}" }
                        }
                    }
                    p { class: "note", "{s::budgets_note()}" }
                }
                if !budgeted.is_empty() {
                    section { class: "card",
                        ul { class: "list budgets", id: "budgeted",
                            for row in budgeted {
                                BudgetRow { key: "{row.id}", row, first, on_saved: move |()| data.restart() }
                            }
                        }
                    }
                }
                if !others.is_empty() {
                    h3 { class: "group-title", "{s::not_budgeted()}" }
                    section { class: "card",
                        ul { class: "list budgets", id: "unbudgeted",
                            for row in others {
                                BudgetRow { key: "{row.id}", row, first, on_saved: move |()| data.restart() }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        h2 { "{s::budgets()}" }
        div { class: "month-nav",
            button {
                class: "secondary",
                id: "prev-month",
                title: s::previous_month(),
                onclick: move |_| {
                    let (y, m) = shown();
                    shown.set(budgets::previous(y, m));
                },
                "‹"
            }
            span { id: "budget-month", "{format::month_year(year, month)}" }
            button {
                class: "secondary",
                id: "next-month",
                title: s::next_month(),
                onclick: move |_| {
                    let (y, m) = shown();
                    shown.set(budgets::next(y, m));
                },
                "›"
            }
        }
        {body}
    }
}

/// The rows with (`true`) or without a budget. A child shows under its parent when
/// the parent is in the same group, and with the parent's name otherwise.
fn group(rows: &[Row], with_budget: bool) -> Vec<Row> {
    let picked: Vec<&Row> = rows
        .iter()
        .filter(|r| r.budget.is_some() == with_budget)
        .collect();
    picked
        .iter()
        .map(|r| {
            let mut r = (*r).clone();
            let parent_here = r.parent.is_some_and(|p| picked.iter().any(|x| x.id == p));
            if parent_here {
                r.title.clone_from(&r.name);
            } else {
                r.depth = 0;
            }
            r
        })
        .collect()
}

#[component]
fn BudgetRow(row: Row, first: time::Date, on_saved: EventHandler<()>) -> Element {
    let store = use_store();
    let mut editing = use_signal(|| false);
    let mut text = use_signal(String::new);
    let mut error = use_signal(|| None::<Problem>);
    let mut busy = use_signal(|| false);
    let new_id = use_hook(api::new_id);
    let category_id = row.id;
    let id = row.budget.as_ref().map_or(new_id, |b| b.id);
    let current = row.budget.as_ref().map(|b| b.data.limit.minor);

    let save = move |e: FormEvent| {
        e.prevent_default();
        let minor = match field(
            s::monthly_limit(),
            parse_amount(&text.read(), CURRENCY, false),
        ) {
            Ok(m) => m,
            Err(msg) => {
                error.set(Some(msg.into()));
                return;
            }
        };
        let data = BudgetData {
            category_id,
            period_month: first,
            limit: billo_api::Money::new(minor, CURRENCY.code()),
        };
        busy.set(true);
        spawn(async move {
            match store
                .call(|c| async move { c.put_budget(id, data).await })
                .await
            {
                Ok(_) => {
                    editing.set(false);
                    on_saved.call(());
                }
                Err(e) => error.set(Some(e.into())),
            }
            busy.set(false);
        });
    };
    let remove = move |_| {
        busy.set(true);
        spawn(async move {
            match store
                .call(|c| async move { c.delete_budget(id).await })
                .await
            {
                Ok(()) => {
                    editing.set(false);
                    on_saved.call(());
                }
                Err(e) => error.set(Some(e.into())),
            }
            busy.set(false);
        });
    };
    let open = move |_| {
        text.set(
            current
                .map(|m| format::plain_amount(m, CURRENCY))
                .unwrap_or_default(),
        );
        error.set(None);
        editing.set(true);
    };

    let over = row.figures.as_ref().is_some_and(|f| f.over_budget);
    let mut class = String::from("budget");
    if row.depth > 0 {
        class.push_str(" child");
    }
    if over {
        class.push_str(" over");
    }
    rsx! {
        li { class: "{class}", "data-category": "{row.name}",
            div { class: "budget-head",
                span { class: "title", "{row.title}" }
                if !editing() {
                    button { class: "link edit-limit", onclick: open,
                        if row.budget.is_some() { "{s::edit()}" } else { "{s::set_limit()}" }
                    }
                }
            }
            if let Some(f) = &row.figures {
                Figures { f: f.clone() }
            }
            if editing() {
                form { class: "limit-form", onsubmit: save,
                    NumberField {
                        id: "budget-limit",
                        label: s::monthly_limit(),
                        value: text(),
                        placeholder: "0.00",
                        oninput: move |v| text.set(v),
                    }
                    if let Some(msg) = error() {
                        ErrorBox { message: msg.message, details: msg.details }
                    }
                    div { class: "actions",
                        button { r#type: "submit", class: "save-limit", disabled: busy(), "{s::save()}" }
                        if row.budget.is_some() {
                            button { r#type: "button", class: "danger remove-limit", disabled: busy(), onclick: remove, "{s::remove_limit()}" }
                        }
                        button { r#type: "button", class: "secondary", onclick: move |_| editing.set(false), "{s::cancel()}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Figures(f: CategoryBudget<Key>) -> Element {
    // Bar width in whole percent, capped at 100; integer arithmetic only.
    let width = f.utilisation_bp.map_or(100, |bp| bp.min(10_000) / 100);
    let percent = f
        .utilisation_bp
        .map(format::whole_percent)
        .unwrap_or_default();
    let left = if f.over_budget {
        s::over_by(&format::money(
            f.remaining.checked_neg().unwrap_or(f.remaining),
        ))
    } else {
        s::left_amount(&format::money(f.remaining))
    };
    rsx! {
        div { class: "bar", role: "presentation",
            div { class: "fill", style: "width: {width}%" }
        }
        div { class: "figures",
            span { class: "spent", "{s::spent_of(&format::money(f.spent), &format::money(f.limit))}" }
            span { class: "pct", "{percent}" }
        }
        div { class: "figures",
            span { class: "left", "{left}" }
            if f.over_budget {
                span { class: "badge overdue", "{s::over_budget()}" }
            }
        }
    }
}
