//! Keeps the signed-in session, the language choice and the user's extra bank holidays
//! across restarts.
//!
//! Web: `localStorage`. Native: a JSON file in the app's data directory.
//! TODO: move the native token to the Keychain (iOS) / Keystore (Android), per
//! architecture §6, before any real release.

use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use crate::strings::Lang;

/// Who is signed in, and the bearer token for the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user_id: Uuid,
    pub email: String,
}

const SESSION: &str = "billo.session";
const LANG: &str = "billo.lang";
const HOLIDAYS: &str = "billo.holidays";

/// The saved session, if any. A corrupt entry counts as signed out.
pub fn load() -> Option<Session> {
    serde_json::from_str(&backend::read(SESSION)?).ok()
}

pub fn save(session: &Session) {
    if let Ok(json) = serde_json::to_string(session) {
        backend::write(SESSION, &json);
    }
}

pub fn clear() {
    backend::remove(SESSION);
}

/// The language the user chose, or the device's: Thai for a `th` locale, otherwise
/// English. Kept when signing out.
pub fn load_lang() -> Lang {
    backend::read(LANG)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(|| Lang::from_locale(&backend::locale().unwrap_or_default()))
}

pub fn save_lang(lang: Lang) {
    if let Ok(json) = serde_json::to_string(&lang) {
        backend::write(LANG, &json);
    }
}

/// Extra bank holidays the user added (on top of the built-in Thai ones), sorted.
/// Device-only for now: not synced, kept when signing out like the language.
pub fn load_holidays() -> Vec<Date> {
    backend::read(HOLIDAYS)
        .map(|json| decode_holidays(&json))
        .unwrap_or_default()
}

pub fn save_holidays(dates: &[Date]) {
    backend::write(HOLIDAYS, &encode_holidays(dates));
}

/// `["2026-10-16", …]`.
fn encode_holidays(dates: &[Date]) -> String {
    let list: Vec<String> = dates.iter().map(|d| iso_date(*d)).collect();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned())
}

/// Unreadable entries are skipped; the result is sorted and without repeats.
fn decode_holidays(json: &str) -> Vec<Date> {
    let list: Vec<String> = serde_json::from_str(json).unwrap_or_default();
    let mut dates: Vec<Date> = list
        .iter()
        .filter_map(|s| crate::input::parse_date(s).ok())
        .collect();
    dates.sort_unstable();
    dates.dedup();
    dates
}

fn iso_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn holidays_round_trip() {
        let dates = [date!(2026 - 10 - 16), date!(2027 - 03 - 10)];
        let json = encode_holidays(&dates);
        assert_eq!(json, r#"["2026-10-16","2027-03-10"]"#);
        assert_eq!(decode_holidays(&json), dates);
    }

    #[test]
    fn bad_holiday_entries_are_skipped() {
        assert_eq!(
            decode_holidays(r#"["2027-03-10","nope","2026-10-16","2026-10-16"]"#),
            [date!(2026 - 10 - 16), date!(2027 - 03 - 10)]
        );
        assert!(decode_holidays("not json").is_empty());
    }
}

#[cfg(target_arch = "wasm32")]
mod backend {
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn read(key: &str) -> Option<String> {
        storage()?.get_item(key).ok()?
    }

    pub fn write(key: &str, value: &str) {
        if let Some(s) = storage() {
            let _ = s.set_item(key, value);
        }
    }

    pub fn remove(key: &str) {
        if let Some(s) = storage() {
            let _ = s.remove_item(key);
        }
    }

    /// The browser's preferred language, e.g. `th-TH`.
    pub fn locale() -> Option<String> {
        web_sys::window()?.navigator().language()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use std::path::PathBuf;

    fn path(key: &str) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("billo")
            .join(format!("{key}.json"))
    }

    pub fn read(key: &str) -> Option<String> {
        std::fs::read_to_string(path(key)).ok()
    }

    pub fn write(key: &str, value: &str) {
        let p = path(key);
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, value);
    }

    pub fn remove(key: &str) {
        let _ = std::fs::remove_file(path(key));
    }

    /// The OS's preferred language, e.g. `th-TH`.
    pub fn locale() -> Option<String> {
        sys_locale::get_locale()
    }
}
