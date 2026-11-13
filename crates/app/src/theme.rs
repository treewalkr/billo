//! The look of the app: the two themes and the self-hosted fonts.
//!
//! A theme is a block of CSS custom properties in `main.css`, picked by `data-theme` on
//! `<html>` and on the app root.
//!
//! The fonts are bundled assets (hashed names, so the service worker keeps them cached
//! for offline use) and declared here because CSS files can't name hashed assets.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::strings as s;

/// The user's chosen look. Kept on the device, like the language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Restrained, like a bank statement. The default.
    #[default]
    Calm,
    /// Softer and more personal.
    Warm,
}

impl Theme {
    pub const ALL: [Self; 2] = [Self::Calm, Self::Warm];

    /// `"calm"` / `"warm"`: the `data-theme` value.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Warm => "warm",
        }
    }

    /// The name in the current language, for the switch.
    pub fn label(self) -> &'static str {
        match self {
            Self::Calm => s::theme_calm(),
            Self::Warm => s::theme_warm(),
        }
    }

    /// The page background, for the browser's `theme-color`.
    pub const fn background(self) -> &'static str {
        match self {
            Self::Calm => "#f4f4f1",
            Self::Warm => "#faf5ee",
        }
    }
}

/// Puts the theme on `<html>`, so the page around the app matches. Called before the
/// first render, so the app doesn't flash in the other theme.
pub fn apply(theme: Theme) {
    #[cfg(target_arch = "wasm32")]
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-theme", theme.code());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = document::eval(&format!(
            "document.documentElement.dataset.theme = \"{}\";",
            theme.code()
        ));
    }
}

const PLEX_400: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-400.woff2");
const PLEX_500: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-500.woff2");
const PLEX_600: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-600.woff2");
const ANUPHAN_400: Asset = asset!("/assets/fonts/anuphan-400.woff2");
const ANUPHAN_500: Asset = asset!("/assets/fonts/anuphan-500.woff2");
const ANUPHAN_600: Asset = asset!("/assets/fonts/anuphan-600.woff2");

/// `@font-face` rules for every bundled font. `font-display: swap` shows the system
/// font until the file has loaded.
pub fn font_faces() -> String {
    let faces = [
        ("IBM Plex Sans Thai", 400, PLEX_400),
        ("IBM Plex Sans Thai", 500, PLEX_500),
        ("IBM Plex Sans Thai", 600, PLEX_600),
        ("Anuphan", 400, ANUPHAN_400),
        ("Anuphan", 500, ANUPHAN_500),
        ("Anuphan", 600, ANUPHAN_600),
    ];
    faces
        .iter()
        .map(|(family, weight, url)| {
            format!(
                "@font-face{{font-family:\"{family}\";font-style:normal;font-weight:{weight};\
                 font-display:swap;src:url(\"{url}\") format(\"woff2\");}}"
            )
        })
        .collect()
}

/// The `@font-face` rules, in the page head.
#[component]
pub fn Fonts() -> Element {
    let css = font_faces();
    rsx! {
        document::Style { "{css}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_round_trips_as_its_code() {
        for t in Theme::ALL {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.code()));
            assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), t);
        }
        assert_eq!(Theme::default(), Theme::Calm);
    }

    #[test]
    fn every_font_has_a_face() {
        let css = font_faces();
        assert_eq!(css.matches("@font-face").count(), 6);
        assert!(css.contains("font-display:swap"));
    }
}
