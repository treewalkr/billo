//! An account's transactions: the list (newest first) and the add/edit form.

use billo_api::{
    AccountData, CategoryData, Record, TransactionData, TransactionKind, TransactionQuery,
};
use billo_domain::{Currency, Money};
use dioxus::prelude::*;

use super::accounts::BackLink;
use super::fields::{NumberField, SelectField, TextField};
use super::{ErrorBox, Loading, Screen, use_auth, use_nav};
use crate::forms::{
    NEW_CATEGORY, TransactionDraft, category_kind, is_card_like, key, transaction_kinds,
};
use crate::{api, categories, clock, format, strings as s};

/// Kinds that add money to a cash account or reduce what's owed on a debt.
fn is_credit(kind: TransactionKind, minor: i64) -> bool {
    match kind {
        TransactionKind::Income | TransactionKind::Payment => true,
        TransactionKind::InterestAdjust => minor < 0,
        _ => false,
    }
}

fn amount_text(m: &billo_api::Money) -> String {
    match Currency::from_code(&m.currency) {
        Some(c) => format::money(Money::new(m.minor, c)),
        None => format!("{} {}", m.minor, m.currency),
    }
}

#[component]
pub fn TransactionList(account: Record<AccountData>) -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let id = account.id;
    let mut txns = use_resource(move || async move {
        let q = TransactionQuery {
            account_id: Some(id),
            ..Default::default()
        };
        let (txns, cats) = auth
            .call(|c| async move {
                let t = c.transactions(&q).await?;
                let cats = c.categories().await?;
                Ok((t, cats))
            })
            .await?;
        Ok::<_, api::ApiError>((txns, cats))
    });
    let add_account = account.clone();
    let body = match &*txns.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), on_retry: move |_| txns.restart() }
        },
        Some(Ok((list, _))) if list.is_empty() => {
            rsx! { p { class: "empty", "{s::NO_TRANSACTIONS}" } }
        }
        Some(Ok((list, cats))) => {
            let mut items: Vec<_> = list.clone();
            // Newest first; the server already sorts, this keeps posting-date ties stable.
            items.sort_by(|a, b| {
                b.data
                    .occurred_on
                    .cmp(&a.data.occurred_on)
                    .then(b.id.cmp(&a.id))
            });
            let rows: Vec<_> = items
                .into_iter()
                .map(|t| {
                    let cat = t
                        .data
                        .category_id
                        .and_then(|c| cats.iter().find(|x| x.id == c))
                        .map(|c| categories::label(cats, &c.data));
                    let mut sub = vec![format::date(t.data.occurred_on)];
                    if let Some(p) = t.data.posted_on.filter(|p| *p != t.data.occurred_on) {
                        sub.push(format!("{} {}", s::POSTED, format::date(p)));
                    }
                    if let Some(c) = cat {
                        sub.push(c);
                    }
                    let title = t.data.note.clone().unwrap_or_else(|| {
                        s::transaction_kind_label(account.data.kind, t.data.kind).to_owned()
                    });
                    let credit = is_credit(t.data.kind, t.data.amount.minor);
                    (t, title, sub.join(" · "), credit)
                })
                .collect();
            rsx! {
                ul { class: "list", id: "transactions",
                    for (t, title, sub, credit) in rows {
                        li {
                            key: "{t.id}",
                            class: "clickable txn",
                            onclick: {
                                let account = account.clone();
                                let t = t.clone();
                                move |_| nav.set(Screen::TransactionForm(Box::new(account.clone()), Some(Box::new(t.clone()))))
                            },
                            div { class: "main",
                                span { class: "title", "{title}" }
                                span { class: "sub", "{sub}" }
                            }
                            span { class: if credit { "amount credit" } else { "amount" }, "{amount_text(&t.data.amount)}" }
                        }
                    }
                }
            }
        }
    };
    rsx! {
        section { class: "card",
            div { class: "card-head",
                h3 { "{s::TRANSACTIONS}" }
                button {
                    class: "secondary",
                    id: "add-transaction",
                    onclick: move |_| nav.set(Screen::TransactionForm(Box::new(add_account.clone()), None)),
                    "{s::ADD}"
                }
            }
            {body}
        }
    }
}

#[component]
pub fn TransactionForm(
    account: Record<AccountData>,
    existing: Option<Record<TransactionData>>,
) -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let today = clock::today();
    let kind = account.data.kind;
    let init = existing.clone();
    let mut draft = use_signal(move || match &init {
        Some(t) => TransactionDraft::from_data(&t.data),
        None => TransactionDraft::new(kind, today),
    });
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut confirm = use_signal(|| false);
    let categories =
        use_resource(
            move || async move { auth.call(|c| async move { c.categories().await }).await },
        );
    let new_id = use_hook(api::new_id);
    let (id, base_version) = match &existing {
        Some(t) => (t.id, Some(t.version)),
        None => (new_id, None),
    };
    let account_id = account.id;
    let currency = account.data.currency.clone();

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let d = draft.read().clone();
        let new_cat = match d.new_category_name() {
            Ok(n) => n,
            Err(msg) => {
                error.set(Some(msg));
                return;
            }
        };
        let new_cat_id = new_cat.as_ref().map(|_| api::new_id());
        let data = match d.to_data(account_id, kind, &currency, new_cat_id) {
            Ok(x) => x,
            Err(msg) => {
                error.set(Some(msg));
                return;
            }
        };
        error.set(None);
        busy.set(true);
        let cat_kind = category_kind(kind, d.kind);
        spawn(async move {
            let result = auth
                .call(|c| async move {
                    if let (Some(name), Some(cid)) = (new_cat, new_cat_id) {
                        let cat = CategoryData {
                            parent_id: None,
                            name_th: None,
                            name_en: Some(name),
                            kind: cat_kind,
                        };
                        c.put_category(cid, None, cat).await?;
                    }
                    c.put_transaction(id, base_version, data).await
                })
                .await;
            match result {
                Ok(_) => nav.set(Screen::Account(account_id)),
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
            match auth
                .call(|c| async move { c.delete_transaction(id).await })
                .await
            {
                Ok(()) => nav.set(Screen::Account(account_id)),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };

    let d = draft.read().clone();
    let kinds: Vec<(String, String)> = transaction_kinds(kind)
        .iter()
        .map(|k| (key(k), s::transaction_kind_label(kind, *k).to_owned()))
        .collect();
    let want = category_kind(kind, d.kind);
    let mut cat_options = vec![(String::new(), s::NO_CATEGORY.to_owned())];
    let cat_error = match &*categories.read() {
        Some(Ok(list)) => {
            cat_options.extend(
                list.iter()
                    .filter(|c| c.data.kind == want || c.id.to_string() == d.category)
                    .map(|c| (c.id.to_string(), categories::label(list, &c.data))),
            );
            None
        }
        Some(Err(e)) => Some(e.to_string()),
        None => None,
    };
    cat_options.push((NEW_CATEGORY.to_owned(), s::NEW_CATEGORY_OPTION.to_owned()));
    let title = if existing.is_some() {
        s::EDIT_TRANSACTION
    } else {
        s::ADD_TRANSACTION
    };
    let amount_hint =
        (d.kind == TransactionKind::InterestAdjust).then(|| s::INTEREST_ADJUST_HINT.to_owned());

    rsx! {
        BackLink { to: Screen::Account(account_id) }
        h2 { "{title}" }
        p { class: "note", "{account.data.name}" }
        form { class: "card", id: "transaction-form", onsubmit: submit,
            SelectField {
                id: "txn-kind",
                label: s::TYPE,
                value: key(&d.kind),
                options: kinds,
                onchange: move |v: String| {
                    if let Some(k) = crate::forms::from_key(&v) { draft.write().kind = k; }
                },
            }
            NumberField {
                id: "amount",
                label: s::AMOUNT,
                value: d.amount.clone(),
                placeholder: "0.00",
                hint: amount_hint,
                oninput: move |v| draft.write().amount = v,
            }
            div { class: "pair",
                TextField {
                    id: "occurred-on",
                    label: s::DATE,
                    input_type: "date",
                    value: d.occurred_on.clone(),
                    oninput: move |v| draft.write().occurred_on = v,
                }
                if is_card_like(kind) {
                    TextField {
                        id: "posted-on",
                        label: s::POSTING_DATE,
                        input_type: "date",
                        value: d.posted_on.clone(),
                        oninput: move |v| draft.write().posted_on = v,
                    }
                }
            }
            if is_card_like(kind) {
                p { class: "hint", "{s::POSTING_DATE_HINT}" }
            }
            SelectField {
                id: "category",
                label: s::CATEGORY,
                value: d.category.clone(),
                options: cat_options,
                onchange: move |v| draft.write().category = v,
            }
            if let Some(msg) = cat_error {
                p { class: "hint error", "{msg}" }
            }
            if d.category == NEW_CATEGORY {
                TextField {
                    id: "new-category",
                    label: s::NEW_CATEGORY_NAME,
                    value: d.new_category.clone(),
                    oninput: move |v| draft.write().new_category = v,
                }
            }
            TextField {
                id: "note",
                label: s::NOTE,
                value: d.note.clone(),
                oninput: move |v| draft.write().note = v,
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg }
            }
            button { r#type: "submit", id: "save-transaction", disabled: busy(), "{s::SAVE}" }
            if existing.is_some() {
                div { class: "actions",
                    button { r#type: "button", class: "danger", id: "delete-transaction", onclick: delete,
                        if confirm() { "{s::CONFIRM_DELETE}" } else { "{s::DELETE}" }
                    }
                }
            }
        }
    }
}
