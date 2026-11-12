//! Turning an annual rate into daily interest without floats.

use time::{Date, Duration};

/// How many days make a year when turning an annual rate into a daily one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DayCount {
    /// Always 365.
    #[default]
    Actual365,
    /// 366 for days in a leap year, 365 otherwise.
    ActualActual,
    /// Always 360.
    Actual360,
}

/// lcm(360, 365, 366): lets every day's share sit over one common denominator.
pub(crate) const LCM: i128 = 1_603_080;

/// Denominator for numerators built with [`DayCount::day_numerator`].
pub(crate) const DENOMINATOR: i128 = 10_000 * LCM;

impl DayCount {
    fn days_in_year(self, date: Date) -> i128 {
        match self {
            Self::Actual365 => 365,
            Self::Actual360 => 360,
            Self::ActualActual => i128::from(time::util::days_in_year(date.year())),
        }
    }

    /// One day's interest on `balance_minor` at `rate_bp` a year, over [`DENOMINATOR`].
    pub(crate) fn day_numerator(self, balance_minor: i128, rate_bp: u32, day: Date) -> i128 {
        balance_minor * i128::from(rate_bp) * (LCM / self.days_in_year(day))
    }

    /// Interest on a constant balance for each day in `from..to` (end exclusive),
    /// over [`DENOMINATOR`].
    pub(crate) fn span_numerator(
        self,
        balance_minor: i128,
        rate_bp: u32,
        from: Date,
        to: Date,
    ) -> i128 {
        let mut n = 0;
        let mut day = from;
        while day < to {
            n += self.day_numerator(balance_minor, rate_bp, day);
            day += Duration::DAY;
        }
        n
    }
}
