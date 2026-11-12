//! Keeps the signed-in session across restarts.
//!
//! Web: `localStorage`. Native: a JSON file in the app's data directory.
//! TODO: move the native token to the Keychain (iOS) / Keystore (Android), per
//! architecture §6, before any real release.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Who is signed in, and the bearer token for the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user_id: Uuid,
    pub email: String,
}

const KEY: &str = "billo.session";

/// The saved session, if any. A corrupt entry counts as signed out.
pub fn load() -> Option<Session> {
    serde_json::from_str(&backend::read()?).ok()
}

pub fn save(session: &Session) {
    if let Ok(json) = serde_json::to_string(session) {
        backend::write(&json);
    }
}

pub fn clear() {
    backend::remove();
}

#[cfg(target_arch = "wasm32")]
mod backend {
    use super::KEY;

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn read() -> Option<String> {
        storage()?.get_item(KEY).ok()?
    }

    pub fn write(value: &str) {
        if let Some(s) = storage() {
            let _ = s.set_item(KEY, value);
        }
    }

    pub fn remove() {
        if let Some(s) = storage() {
            let _ = s.remove_item(KEY);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use std::path::PathBuf;

    use super::KEY;

    fn path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("billo")
            .join(format!("{KEY}.json"))
    }

    pub fn read() -> Option<String> {
        std::fs::read_to_string(path()).ok()
    }

    pub fn write(value: &str) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, value);
    }

    pub fn remove() {
        let _ = std::fs::remove_file(path());
    }
}
