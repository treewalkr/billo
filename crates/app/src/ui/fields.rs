//! Labelled form controls.

use dioxus::prelude::*;

#[component]
pub fn TextField(
    id: &'static str,
    label: &'static str,
    value: String,
    oninput: EventHandler<String>,
    /// `text` (default), `date`, `email`…
    input_type: Option<&'static str>,
    /// `decimal` or `numeric` shows a number pad on phones.
    inputmode: Option<&'static str>,
    placeholder: Option<&'static str>,
    hint: Option<String>,
) -> Element {
    rsx! {
        div { class: "field",
            label { r#for: id, "{label}" }
            input {
                id,
                r#type: input_type.unwrap_or("text"),
                inputmode: inputmode.unwrap_or(""),
                placeholder: placeholder.unwrap_or(""),
                value,
                oninput: move |e| oninput.call(e.value()),
            }
            if let Some(h) = hint {
                p { class: "hint", "{h}" }
            }
        }
    }
}

/// A number typed as text (`inputmode="decimal"`), so `1,234.56` is accepted and
/// parsed with integer arithmetic.
#[component]
pub fn NumberField(
    id: &'static str,
    label: &'static str,
    value: String,
    oninput: EventHandler<String>,
    placeholder: Option<&'static str>,
    hint: Option<String>,
) -> Element {
    rsx! {
        TextField { id, label, value, oninput, inputmode: "decimal", placeholder, hint }
    }
}

#[component]
pub fn SelectField(
    id: &'static str,
    label: &'static str,
    value: String,
    /// `(value, label)` pairs.
    options: Vec<(String, String)>,
    onchange: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "field",
            label { r#for: id, "{label}" }
            select { id, value: "{value}", onchange: move |e| onchange.call(e.value()),
                for (v, l) in options {
                    option { value: "{v}", selected: v == value, "{l}" }
                }
            }
        }
    }
}

#[component]
pub fn CheckField(
    id: &'static str,
    label: &'static str,
    checked: bool,
    onchange: EventHandler<bool>,
) -> Element {
    rsx! {
        div { class: "check",
            input {
                id,
                r#type: "checkbox",
                checked,
                onchange: move |e| onchange.call(e.checked()),
            }
            label { r#for: id, "{label}" }
        }
    }
}
