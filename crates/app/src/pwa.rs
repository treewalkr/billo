//! Web only: the installable app (manifest, icons) and the service worker that loads
//! the app shell with no network (`public/sw.js`, which explains its caching).

use dioxus::prelude::*;

/// `0.1.0-1`: names the service worker's cache, so a new release replaces it.
pub fn cache_version() -> String {
    format!("{}-{}", env!("CARGO_PKG_VERSION"), env!("BILLO_BUILD"))
}

/// Registers `/sw.js` and hands it the URLs this page already loaded (the worker
/// wasn't in control yet), so the first online visit is enough to start offline.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn register_service_worker() {
    let js = format!(
        r#"
        if ("serviceWorker" in navigator) {{
            navigator.serviceWorker
                .register("/sw.js?v={v}", {{ scope: "/" }})
                .then(() => navigator.serviceWorker.ready)
                .then((reg) => {{
                    const urls = [location.href].concat(
                        performance.getEntriesByType("resource").map((e) => e.name),
                    );
                    if (reg.active) reg.active.postMessage({{ type: "cache-urls", urls }});
                }})
                .catch((e) => console.warn("service worker not registered", e));
        }}
        "#,
        v = cache_version()
    );
    let _ = document::eval(&js);
}

/// Manifest and icons, for the page head (the theme colour follows the theme).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[component]
pub fn WebAppHead() -> Element {
    use_hook(register_service_worker);
    rsx! {
        // Draw under the notch and home indicator; the CSS pads with the safe-area insets.
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        document::Link { rel: "manifest", href: "/manifest.webmanifest" }
        document::Link { rel: "icon", r#type: "image/png", href: "/icon-192.png" }
        document::Link { rel: "apple-touch-icon", href: "/icon-192.png" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_version_is_version_and_build() {
        let v = cache_version();
        let (version, build) = v.rsplit_once('-').unwrap();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(build.parse::<u32>().unwrap() >= 1);
    }
}
