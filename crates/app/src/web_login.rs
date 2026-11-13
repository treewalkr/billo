//! The end of the web Google sign-in: the server sends the browser back to the app
//! with the result in the URL fragment, `#billo_token=…&user_id=…&email=…` or
//! `#billo_error=<code>`. Fragments never reach a server, so the token stays out of
//! logs; the app reads it once at start and removes it from the address bar.

use uuid::Uuid;

use crate::storage::Session;

/// What the server handed back.
// Only built from a URL on the web; the type is shared so the app compiles the same.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Returned {
    Session(Session),
    /// A `billo_error` code (`cancelled`, `expired`, `email_unverified`, …).
    Error(String),
}

/// Parses a fragment (with or without the leading `#`). `None` when it isn't a
/// sign-in result, so unrelated fragments are left alone.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn parse_fragment(fragment: &str) -> Option<Returned> {
    let fragment = fragment.strip_prefix('#').unwrap_or(fragment);
    let mut token = None;
    let mut user_id = None;
    let mut email = None;
    let mut error = None;
    for (k, v) in form_urlencoded::parse(fragment.as_bytes()) {
        match &*k {
            "billo_token" => token = Some(v.into_owned()),
            "user_id" => user_id = Some(v.into_owned()),
            "email" => email = Some(v.into_owned()),
            "billo_error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if let Some(code) = error {
        return Some(Returned::Error(code));
    }
    let token = token.filter(|t| !t.is_empty())?;
    match user_id.as_deref().map(str::parse::<Uuid>) {
        Some(Ok(user_id)) => Some(Returned::Session(Session {
            token,
            user_id,
            email: email.unwrap_or_default(),
        })),
        // A token without a usable user id can't be used safely.
        _ => Some(Returned::Error("invalid_response".to_owned())),
    }
}

/// Reads and removes a sign-in result from the address bar (web only).
pub fn take() -> Option<Returned> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let location = window.location();
        let returned = parse_fragment(&location.hash().ok()?)?;
        // Drop the fragment without a reload or a new history entry.
        let clean = format!(
            "{}{}",
            location.pathname().unwrap_or_default(),
            location.search().unwrap_or_default()
        );
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&clean));
        }
        Some(returned)
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The current page's origin, e.g. `http://localhost:8081` (web only).
pub fn current_origin() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    return web_sys::window()?.location().origin().ok();
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// Leaves the app for `url` (web only).
pub fn navigate(url: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href(url);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = url;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_session() {
        let id = "0190a4b2-7c3d-7e4f-8a1b-2c3d4e5f6a7b";
        let got = parse_fragment(&format!(
            "#billo_token=abc-_123&user_id={id}&email=me%40example.com"
        ));
        assert_eq!(
            got,
            Some(Returned::Session(Session {
                token: "abc-_123".into(),
                user_id: id.parse().unwrap(),
                email: "me@example.com".into(),
            }))
        );
    }

    #[test]
    fn reads_an_error() {
        assert_eq!(
            parse_fragment("billo_error=cancelled"),
            Some(Returned::Error("cancelled".into()))
        );
    }

    #[test]
    fn ignores_other_fragments() {
        assert_eq!(parse_fragment(""), None);
        assert_eq!(parse_fragment("#"), None);
        assert_eq!(parse_fragment("#section-2"), None);
        assert_eq!(parse_fragment("#billo_token="), None);
    }

    #[test]
    fn token_without_user_id_is_an_error() {
        assert_eq!(
            parse_fragment("#billo_token=abc&user_id=nope"),
            Some(Returned::Error("invalid_response".into()))
        );
    }
}
