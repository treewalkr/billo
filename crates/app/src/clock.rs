//! Today's date in the device's time zone.

use time::Date;

#[cfg(target_arch = "wasm32")]
pub fn today() -> Date {
    let now = js_sys::Date::new_0();
    // Integer-valued doubles; casting is not float arithmetic.
    let year = now.get_full_year() as i32;
    let month = time::Month::try_from(now.get_month() as u8 + 1).unwrap_or(time::Month::January);
    let day = now.get_date() as u8;
    Date::from_calendar_date(year, month, day).unwrap_or(Date::MIN)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn today() -> Date {
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date()
}
