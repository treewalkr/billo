//! Categories: the list (spending and income, as a two-level tree), the add/edit form,
//! and the one-tap starter set for new users.

use billo_api::{CategoryData, CategoryKind, Record};
use dioxus::prelude::*;

use super::accounts::BackLink;
use super::fields::{SelectField, TextField};
use super::{ErrorBox, Loading, Screen, use_auth, use_nav};
use crate::categories::{self, parent_options, starter_records, tree};
use crate::forms::{CategoryDraft, from_key, key};
use crate::{api, strings as s};

const KINDS: [CategoryKind; 2] = [CategoryKind::Expense, CategoryKind::Income];

#[component]
pub fn Categories() -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let mut list =
        use_resource(
            move || async move { auth.call(|c| async move { c.categories().await }).await },
        );

    let body = match &*list.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: e.to_string(), on_retry: move |_| list.restart() }
        },
        Some(Ok(all)) if all.is_empty() => rsx! {
            StarterOffer { on_added: move |()| list.restart() }
            p { class: "empty", "{s::no_categories()}" }
        },
        Some(Ok(all)) => {
            let groups: Vec<_> = KINDS
                .iter()
                .map(|k| (*k, tree(all, *k)))
                .filter(|(_, items)| !items.is_empty())
                .collect();
            rsx! {
                for (kind, items) in groups {
                    h3 { class: "group-title", "{s::category_kind_label(kind)}" }
                    section { class: "card",
                        ul { class: "list", id: "categories-{key(&kind)}",
                            for (c, depth) in items {
                                li {
                                    key: "{c.id}",
                                    class: if depth > 0 { "clickable category child" } else { "clickable category" },
                                    onclick: {
                                        let c = c.clone();
                                        move |_| nav.set(Screen::CategoryForm(Some(Box::new(c.clone()))))
                                    },
                                    div { class: "main",
                                        span { class: "title", "{categories::name(&c.data)}" }
                                        if let (Some(th), Some(_)) = (&c.data.name_th, &c.data.name_en) {
                                            span { class: "sub", "{th}" }
                                        }
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
        BackLink { to: Screen::More }
        div { class: "toolbar",
            h2 { "{s::categories()}" }
            button { id: "add-category", onclick: move |_| nav.set(Screen::CategoryForm(None)), "{s::add_category()}" }
        }
        {body}
    }
}

/// Offers the starter categories. Shown only while the user has none.
#[component]
pub fn StarterOffer(on_added: EventHandler<()>, on_dismiss: Option<EventHandler<()>>) -> Element {
    let auth = use_auth();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let add = move |_| {
        busy.set(true);
        error.set(None);
        spawn(async move {
            let result = auth
                .call(|c| async move {
                    // Parents come first, so children can point at them.
                    for (id, data) in starter_records(api::new_id) {
                        c.put_category(id, None, data).await?;
                    }
                    Ok(())
                })
                .await;
            busy.set(false);
            match result {
                Ok(()) => on_added.call(()),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    rsx! {
        section { class: "card starter", id: "starter-offer",
            h3 { "{s::starter_title()}" }
            p { class: "note", "{s::starter_body()}" }
            if let Some(msg) = error() {
                ErrorBox { message: msg }
            }
            div { class: "actions",
                button { id: "add-starter", disabled: busy(), onclick: add,
                    if busy() { "{s::adding()}" } else { "{s::add_starter()}" }
                }
                if let Some(dismiss) = on_dismiss {
                    button { class: "secondary", id: "dismiss-starter", onclick: move |_| dismiss.call(()), "{s::not_now()}" }
                }
            }
        }
    }
}

#[component]
pub fn CategoryForm(existing: Option<Record<CategoryData>>) -> Element {
    let auth = use_auth();
    let mut nav = use_nav();
    let init = existing.clone();
    let mut draft = use_signal(move || match &init {
        Some(c) => CategoryDraft::from_data(&c.data),
        None => CategoryDraft::new(CategoryKind::Expense),
    });
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut confirm = use_signal(|| false);
    let all =
        use_resource(
            move || async move { auth.call(|c| async move { c.categories().await }).await },
        );
    let new_id = use_hook(api::new_id);
    let (id, base_version) = match &existing {
        Some(c) => (c.id, Some(c.version)),
        None => (new_id, None),
    };
    let this = existing.as_ref().map(|c| c.id);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let data = match draft.read().to_data() {
            Ok(d) => d,
            Err(msg) => {
                error.set(Some(msg));
                return;
            }
        };
        error.set(None);
        busy.set(true);
        spawn(async move {
            match auth
                .call(|c| async move { c.put_category(id, base_version, data).await })
                .await
            {
                Ok(_) => nav.set(Screen::Categories),
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
        // Children move to the top level rather than pointing at a deleted parent.
        let children: Vec<_> = all
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|l| {
                l.iter()
                    .filter(|c| c.data.parent_id == Some(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        spawn(async move {
            match auth
                .call(|c| async move {
                    for child in children {
                        let data = CategoryData {
                            parent_id: None,
                            ..child.data
                        };
                        c.put_category(child.id, Some(child.version), data).await?;
                    }
                    c.delete_category(id).await
                })
                .await
            {
                Ok(()) => nav.set(Screen::Categories),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };

    let d = draft.read().clone();
    let kinds: Vec<(String, String)> = KINDS
        .iter()
        .map(|k| (key(k), s::category_kind_label(*k).to_owned()))
        .collect();
    let mut parents = vec![(String::new(), s::no_parent().to_owned())];
    if let Some(Ok(list)) = &*all.read() {
        parents.extend(
            parent_options(list, d.kind, this)
                .iter()
                .map(|c| (c.id.to_string(), categories::name(&c.data))),
        );
    }
    let has_children = this.is_some_and(|id| {
        all.read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .is_some_and(|l| l.iter().any(|c| c.data.parent_id == Some(id)))
    });
    let title = if existing.is_some() {
        s::edit_category()
    } else {
        s::add_category()
    };

    rsx! {
        BackLink { to: Screen::Categories }
        h2 { "{title}" }
        form { class: "card", id: "category-form", onsubmit: submit,
            TextField {
                id: "name-th",
                label: s::name_th(),
                value: d.name_th.clone(),
                oninput: move |v| draft.write().name_th = v,
            }
            TextField {
                id: "name-en",
                label: s::name_en(),
                value: d.name_en.clone(),
                hint: s::category_name_hint().to_owned(),
                oninput: move |v| draft.write().name_en = v,
            }
            if !has_children {
                SelectField {
                    id: "category-kind",
                    label: s::type_label(),
                    value: key(&d.kind),
                    options: kinds,
                    onchange: move |v: String| {
                        if let Some(k) = from_key(&v) {
                            let mut d = draft.write();
                            if d.kind != k {
                                d.kind = k;
                                d.parent.clear();
                            }
                        }
                    },
                }
                SelectField {
                    id: "category-parent",
                    label: s::parent(),
                    value: d.parent.clone(),
                    options: parents,
                    onchange: move |v| draft.write().parent = v,
                }
            }
            if let Some(msg) = error() {
                ErrorBox { message: msg }
            }
            button { r#type: "submit", id: "save-category", disabled: busy(), "{s::save()}" }
            if existing.is_some() {
                p { class: "hint", "{s::category_delete_note()}" }
                div { class: "actions",
                    button { r#type: "button", class: "danger", id: "delete-category", onclick: delete,
                        if confirm() { "{s::confirm_delete()}" } else { "{s::delete()}" }
                    }
                }
            }
        }
    }
}
