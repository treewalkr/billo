//! Business days and month arithmetic.
//!
//! Holidays are data passed in by the caller; the domain never looks them up itself.

use std::collections::BTreeSet;

use time::{Date, Duration, Month, Weekday};

/// Decides whether a bank processes payments on a given day.
pub trait BusinessCalendar {
    fn is_business_day(&self, date: Date) -> bool;

    /// `date` itself if it is a business day, otherwise the next one.
    fn next_business_day(&self, date: Date) -> Date {
        let mut d = date;
        while !self.is_business_day(d) {
            d += Duration::DAY;
        }
        d
    }

    /// `date` itself if it is a business day, otherwise the previous one.
    fn previous_business_day(&self, date: Date) -> Date {
        let mut d = date;
        while !self.is_business_day(d) {
            d -= Duration::DAY;
        }
        d
    }
}

/// Saturday and Sunday are closed; every other day is open. Optional extra holidays.
#[derive(Debug, Clone, Default)]
pub struct WeekendCalendar {
    holidays: BTreeSet<Date>,
}

impl WeekendCalendar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_holidays(holidays: impl IntoIterator<Item = Date>) -> Self {
        Self {
            holidays: holidays.into_iter().collect(),
        }
    }

    /// Weekends plus Thai bank holidays ([`crate::holidays::th`]) plus `extra` days
    /// (e.g. ones the user added).
    pub fn thai(extra: impl IntoIterator<Item = Date>) -> Self {
        Self::with_holidays(crate::holidays::th::dates().chain(extra))
    }
}

impl BusinessCalendar for WeekendCalendar {
    fn is_business_day(&self, date: Date) -> bool {
        !matches!(date.weekday(), Weekday::Saturday | Weekday::Sunday)
            && !self.holidays.contains(&date)
    }
}

/// Every day is a business day (no shifting).
#[derive(Debug, Clone, Copy, Default)]
pub struct EveryDay;

impl BusinessCalendar for EveryDay {
    fn is_business_day(&self, _date: Date) -> bool {
        true
    }
}

/// `day` of the given month, clamped to the month's last day (31 → 28/29 in February).
pub fn clamped_date(year: i32, month: Month, day: u8) -> Date {
    let day = day.min(month.length(year));
    Date::from_calendar_date(year, month, day).expect("day is clamped to month length")
}

/// The (year, month) `n` months after the given one.
pub fn add_months(year: i32, month: Month, n: i32) -> (i32, Month) {
    let index = year * 12 + (month as i32 - 1) + n;
    let month = Month::try_from((index.rem_euclid(12) + 1) as u8).expect("1..=12");
    (index.div_euclid(12), month)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn weekend_shifts_forward_and_back() {
        let cal = WeekendCalendar::new();
        // 2026-11-14 is a Saturday.
        assert_eq!(
            cal.next_business_day(date!(2026 - 11 - 14)),
            date!(2026 - 11 - 16)
        );
        assert_eq!(
            cal.previous_business_day(date!(2026 - 11 - 14)),
            date!(2026 - 11 - 13)
        );
        assert_eq!(
            cal.next_business_day(date!(2026 - 11 - 12)),
            date!(2026 - 11 - 12)
        );
    }

    #[test]
    fn holidays_are_skipped() {
        // Mon 2026-12-07 as a holiday after a weekend.
        let cal = WeekendCalendar::with_holidays([date!(2026 - 12 - 07)]);
        assert_eq!(
            cal.next_business_day(date!(2026 - 12 - 05)),
            date!(2026 - 12 - 08)
        );
    }

    #[test]
    fn thai_holidays_and_weekend_shift_together() {
        let cal = WeekendCalendar::thai([]);
        // Sat 11 + Sun 12 April 2026, then Songkran Mon 13 – Wed 15.
        assert_eq!(
            cal.next_business_day(date!(2026 - 04 - 11)),
            date!(2026 - 04 - 16)
        );
        assert_eq!(
            cal.next_business_day(date!(2026 - 04 - 14)),
            date!(2026 - 04 - 16)
        );
        assert_eq!(
            cal.previous_business_day(date!(2026 - 04 - 15)),
            date!(2026 - 04 - 10)
        );
        // Sat 5 + Sun 6 December 2026, then the substitute holiday Mon 7.
        assert_eq!(
            cal.next_business_day(date!(2026 - 12 - 05)),
            date!(2026 - 12 - 08)
        );
        // Thu 31 Dec 2026, Fri 1 Jan 2027, weekend → Mon 4 Jan 2027.
        assert_eq!(
            cal.next_business_day(date!(2026 - 12 - 31)),
            date!(2027 - 01 - 04)
        );
        // A user's extra day (16 Oct 2026, Bangkok only) is added on top.
        assert!(cal.is_business_day(date!(2026 - 10 - 16)));
        let bkk = WeekendCalendar::thai([date!(2026 - 10 - 16)]);
        assert_eq!(
            bkk.next_business_day(date!(2026 - 10 - 16)),
            date!(2026 - 10 - 19)
        );
    }

    #[test]
    fn clamps_to_month_end() {
        assert_eq!(
            clamped_date(2026, Month::February, 31),
            date!(2026 - 02 - 28)
        );
        assert_eq!(
            clamped_date(2028, Month::February, 31),
            date!(2028 - 02 - 29)
        );
        assert_eq!(clamped_date(2026, Month::April, 31), date!(2026 - 04 - 30));
    }

    #[test]
    fn adds_months_across_years() {
        assert_eq!(add_months(2026, Month::December, 1), (2027, Month::January));
        assert_eq!(
            add_months(2026, Month::January, -1),
            (2025, Month::December)
        );
        assert_eq!(add_months(2026, Month::March, 0), (2026, Month::March));
    }
}
