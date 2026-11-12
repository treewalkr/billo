//! Statement cycles: which statement a purchase lands on, and when it is due.
//!
//! - The statement (cut-off) date is a fixed day of the month, clamped to month end
//!   (day 31 → Feb 28/29, Apr 30). It never shifts for weekends or holidays.
//! - A purchase dated on the cut-off day belongs to that statement.
//! - The due date follows a [`DueRule`], then optionally shifts off non-business days.

use std::fmt;

use time::{Date, Duration, Month};

use crate::calendar::{BusinessCalendar, add_months, clamped_date};

/// How the payment due date is set from the statement date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueRule {
    /// A fixed number of days after the statement date (e.g. 20).
    DaysAfterStatement(u16),
    /// The first occurrence of this day of the month after the statement date,
    /// clamped to month end.
    DayOfMonth(u8),
}

/// What happens when the due date lands on a non-business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DueShift {
    /// Keep the date as is.
    #[default]
    None,
    /// Move to the next business day.
    Next,
    /// Move to the previous business day.
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleError {
    /// Day of month must be 1..=31.
    InvalidDay(u8),
    /// Days after statement must be at least 1.
    ZeroDueDays,
}

impl fmt::Display for CycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDay(d) => write!(f, "day of month must be 1..=31, got {d}"),
            Self::ZeroDueDays => f.write_str("due days after statement must be at least 1"),
        }
    }
}

impl std::error::Error for CycleError {}

/// A card's statement cycle settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleRules {
    statement_day: u8,
    due: DueRule,
    due_shift: DueShift,
}

/// One statement cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementPeriod {
    /// First day covered (the day after the previous statement date).
    pub start: Date,
    /// Statement (cut-off) date; the last day covered.
    pub statement_date: Date,
    /// Payment due date, after any business-day shift.
    pub due_date: Date,
}

impl StatementPeriod {
    /// Whether a purchase on `date` belongs to this statement.
    pub fn contains(&self, date: Date) -> bool {
        self.start <= date && date <= self.statement_date
    }
}

impl CycleRules {
    pub fn new(statement_day: u8, due: DueRule, due_shift: DueShift) -> Result<Self, CycleError> {
        check_day(statement_day)?;
        match due {
            DueRule::DaysAfterStatement(0) => return Err(CycleError::ZeroDueDays),
            DueRule::DayOfMonth(d) => check_day(d)?,
            DueRule::DaysAfterStatement(_) => {}
        }
        Ok(Self {
            statement_day,
            due,
            due_shift,
        })
    }

    pub const fn statement_day(&self) -> u8 {
        self.statement_day
    }

    pub const fn due(&self) -> DueRule {
        self.due
    }

    pub const fn due_shift(&self) -> DueShift {
        self.due_shift
    }

    /// The statement whose cut-off falls in the given month.
    pub fn period_for_month(
        &self,
        year: i32,
        month: Month,
        cal: &impl BusinessCalendar,
    ) -> StatementPeriod {
        let statement_date = clamped_date(year, month, self.statement_day);
        let (py, pm) = add_months(year, month, -1);
        let start = clamped_date(py, pm, self.statement_day) + Duration::DAY;
        let due_date = self.shift(self.raw_due_date(statement_date), cal);
        StatementPeriod {
            start,
            statement_date,
            due_date,
        }
    }

    /// The statement a purchase on `date` lands on.
    pub fn period_containing(&self, date: Date, cal: &impl BusinessCalendar) -> StatementPeriod {
        let this_month = clamped_date(date.year(), date.month(), self.statement_day);
        if date <= this_month {
            self.period_for_month(date.year(), date.month(), cal)
        } else {
            let (y, m) = add_months(date.year(), date.month(), 1);
            self.period_for_month(y, m, cal)
        }
    }

    fn raw_due_date(&self, statement_date: Date) -> Date {
        match self.due {
            DueRule::DaysAfterStatement(n) => statement_date + Duration::days(i64::from(n)),
            DueRule::DayOfMonth(day) => {
                let same = clamped_date(statement_date.year(), statement_date.month(), day);
                if same > statement_date {
                    same
                } else {
                    let (y, m) = add_months(statement_date.year(), statement_date.month(), 1);
                    clamped_date(y, m, day)
                }
            }
        }
    }

    fn shift(&self, date: Date, cal: &impl BusinessCalendar) -> Date {
        match self.due_shift {
            DueShift::None => date,
            DueShift::Next => cal.next_business_day(date),
            DueShift::Previous => cal.previous_business_day(date),
        }
    }
}

fn check_day(day: u8) -> Result<(), CycleError> {
    if (1..=31).contains(&day) {
        Ok(())
    } else {
        Err(CycleError::InvalidDay(day))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{EveryDay, WeekendCalendar};
    use time::macros::date;

    fn rules(day: u8, due: DueRule) -> CycleRules {
        CycleRules::new(day, due, DueShift::None).unwrap()
    }

    #[test]
    fn purchase_on_cutoff_day_is_on_that_statement() {
        let r = rules(25, DueRule::DaysAfterStatement(20));
        let p = r.period_containing(date!(2026 - 11 - 25), &EveryDay);
        assert_eq!(p.statement_date, date!(2026 - 11 - 25));
        assert_eq!(p.start, date!(2026 - 10 - 26));
        assert!(p.contains(date!(2026 - 10 - 26)));
    }

    #[test]
    fn purchase_after_cutoff_moves_to_next_statement() {
        let r = rules(25, DueRule::DaysAfterStatement(20));
        let p = r.period_containing(date!(2026 - 11 - 26), &EveryDay);
        assert_eq!(p.start, date!(2026 - 11 - 26));
        assert_eq!(p.statement_date, date!(2026 - 12 - 25));
    }

    #[test]
    fn december_purchase_rolls_into_january() {
        let r = rules(20, DueRule::DayOfMonth(5));
        let p = r.period_containing(date!(2026 - 12 - 28), &EveryDay);
        assert_eq!(p.statement_date, date!(2027 - 01 - 20));
        assert_eq!(p.due_date, date!(2027 - 02 - 05));
    }

    #[test]
    fn month_end_statement_day_clamps() {
        let r = rules(31, DueRule::DaysAfterStatement(15));
        let feb = r.period_for_month(2026, Month::February, &EveryDay);
        assert_eq!(feb.start, date!(2026 - 02 - 01));
        assert_eq!(feb.statement_date, date!(2026 - 02 - 28));
        let mar = r.period_for_month(2026, Month::March, &EveryDay);
        assert_eq!(mar.start, date!(2026 - 03 - 01));
        assert_eq!(mar.statement_date, date!(2026 - 03 - 31));
        let leap = r.period_for_month(2028, Month::February, &EveryDay);
        assert_eq!(leap.statement_date, date!(2028 - 02 - 29));
    }

    #[test]
    fn periods_are_contiguous_through_short_months() {
        // Day 30: Jan 30 → Feb 28 → Mar 30. Every day must belong to exactly one period.
        let r = rules(30, DueRule::DaysAfterStatement(20));
        let jan = r.period_for_month(2026, Month::January, &EveryDay);
        let feb = r.period_for_month(2026, Month::February, &EveryDay);
        let mar = r.period_for_month(2026, Month::March, &EveryDay);
        assert_eq!(feb.start, jan.statement_date + Duration::DAY);
        assert_eq!(mar.start, feb.statement_date + Duration::DAY);
    }

    #[test]
    fn due_day_of_month_before_statement_day_is_next_month() {
        let r = rules(25, DueRule::DayOfMonth(10));
        let p = r.period_for_month(2026, Month::November, &EveryDay);
        assert_eq!(p.due_date, date!(2026 - 12 - 10));
    }

    #[test]
    fn due_day_of_month_after_statement_day_is_same_month() {
        let r = rules(5, DueRule::DayOfMonth(25));
        let p = r.period_for_month(2026, Month::November, &EveryDay);
        assert_eq!(p.due_date, date!(2026 - 11 - 25));
    }

    #[test]
    fn due_day_equal_to_statement_day_is_next_month() {
        let r = rules(15, DueRule::DayOfMonth(15));
        let p = r.period_for_month(2026, Month::November, &EveryDay);
        assert_eq!(p.due_date, date!(2026 - 12 - 15));
    }

    #[test]
    fn weekend_due_date_shifts() {
        // Statement 2026-10-25 + 20 days = Sat 2026-11-14.
        let cal = WeekendCalendar::new();
        let next = CycleRules::new(25, DueRule::DaysAfterStatement(20), DueShift::Next).unwrap();
        let prev =
            CycleRules::new(25, DueRule::DaysAfterStatement(20), DueShift::Previous).unwrap();
        let none = rules(25, DueRule::DaysAfterStatement(20));
        assert_eq!(
            next.period_for_month(2026, Month::October, &cal).due_date,
            date!(2026 - 11 - 16)
        );
        assert_eq!(
            prev.period_for_month(2026, Month::October, &cal).due_date,
            date!(2026 - 11 - 13)
        );
        assert_eq!(
            none.period_for_month(2026, Month::October, &cal).due_date,
            date!(2026 - 11 - 14)
        );
    }

    #[test]
    fn holiday_due_date_shifts_past_holidays_and_weekend() {
        let cal = WeekendCalendar::thai([]);
        // Statement 25 March 2026, due on the 13th: Mon 13 April 2026 is Songkran
        // (13–15), so the next business day is Thu 16 April.
        let next = CycleRules::new(25, DueRule::DayOfMonth(13), DueShift::Next).unwrap();
        let prev = CycleRules::new(25, DueRule::DayOfMonth(13), DueShift::Previous).unwrap();
        assert_eq!(
            next.period_for_month(2026, Month::March, &cal).due_date,
            date!(2026 - 04 - 16)
        );
        // Back past the weekend of 11–12 April.
        assert_eq!(
            prev.period_for_month(2026, Month::March, &cal).due_date,
            date!(2026 - 04 - 10)
        );
        // Due Sat 5 December 2026: weekend, then the substitute holiday Mon 7.
        let dec = CycleRules::new(15, DueRule::DaysAfterStatement(20), DueShift::Next).unwrap();
        assert_eq!(
            dec.period_for_month(2026, Month::November, &cal).due_date,
            date!(2026 - 12 - 08)
        );
        // Weekends only: the same due date only skips the weekend.
        assert_eq!(
            dec.period_for_month(2026, Month::November, &WeekendCalendar::new())
                .due_date,
            date!(2026 - 12 - 07)
        );
    }

    #[test]
    fn rejects_invalid_rules() {
        assert_eq!(
            CycleRules::new(0, DueRule::DaysAfterStatement(20), DueShift::None),
            Err(CycleError::InvalidDay(0))
        );
        assert_eq!(
            CycleRules::new(25, DueRule::DayOfMonth(32), DueShift::None),
            Err(CycleError::InvalidDay(32))
        );
        assert_eq!(
            CycleRules::new(25, DueRule::DaysAfterStatement(0), DueShift::None),
            Err(CycleError::ZeroDueDays)
        );
    }
}
