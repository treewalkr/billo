//! The look of the app: the self-hosted fonts.
//!
//! The fonts are bundled assets (hashed names, so the service worker keeps them cached
//! for offline use) and declared here because CSS files can't name hashed assets.

use dioxus::prelude::*;

const PLEX_400: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-400.woff2");
const PLEX_500: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-500.woff2");
const PLEX_600: Asset = asset!("/assets/fonts/ibm-plex-sans-thai-600.woff2");

/// `@font-face` rules for every bundled font. `font-display: swap` shows the system
/// font until the file has loaded.
pub fn font_faces() -> String {
    let faces = [
        ("IBM Plex Sans Thai", 400, PLEX_400),
        ("IBM Plex Sans Thai", 500, PLEX_500),
        ("IBM Plex Sans Thai", 600, PLEX_600),
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
