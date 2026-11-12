//! Card interest: daily accrual on the outstanding balance. Every number is a user setting.
//!
//! Conventions:
//! - Interest accrues on each day's **end-of-day** balance. A charge counts on its
//!   effective day; a payment stops interest from its posting day.
//! - A charge's effective day is its transaction date or posting date ([`InterestStart`]).
//!   A charge made before the window but posted inside it accrues from its effective day.
//! - Credit (negative) balances earn nothing.
//! - The exact total is rounded once, at the end, with the card's [`Rounding`].
//! - Whether interest is charged at all (grace when the last statement was paid in full)
//!   is decided by the caller with [`InterestRules::grace_applies`].

use std::fmt;

use time::{Date, Duration};

use crate::daycount::DENOMINATOR;
pub use crate::daycount::DayCount;

use crate::money::{Currency, Money, MoneyError};
use crate::rounding::Rounding;

/// Which date a charge starts accruing interest from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterestStart {
    #[default]
    TransactionDate,
    PostingDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterestError {
    /// Annual rate must be 0..=10_000 basis points (0%..=100%).
    InvalidRate(u32),
    /// The window ends before it starts.
    InvalidWindow,
    /// An entry amount must be positive; its kind gives the direction.
    NonPositiveEntry,
    Money(MoneyError),
}

impl From<MoneyError> for InterestError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for InterestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRate(bp) => {
                write!(f, "annual rate must be 0..=10000 basis points, got {bp}")
            }
            Self::InvalidWindow => f.write_str("interest window ends before it starts"),
            Self::NonPositiveEntry => f.write_str("entry amount must be positive"),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for InterestError {}

/// A card's interest setting, e.g. "16% a year, Actual/365, from the transaction date".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterestRules {
    annual_rate_bp: u32,
    day_count: DayCount,
    start: InterestStart,
    rounding: Rounding,
    grace_if_paid_in_full: bool,
}

impl InterestRules {
    /// `annual_rate_bp` in basis points: 1600 = 16% a year.
    pub fn new(
        annual_rate_bp: u32,
        day_count: DayCount,
        start: InterestStart,
        rounding: Rounding,
        grace_if_paid_in_full: bool,
    ) -> Result<Self, InterestError> {
        if annual_rate_bp > 10_000 {
            return Err(InterestError::InvalidRate(annual_rate_bp));
        }
        Ok(Self {
            annual_rate_bp,
            day_count,
            start,
            rounding,
            grace_if_paid_in_full,
        })
    }

    pub const fn annual_rate_bp(&self) -> u32 {
        self.annual_rate_bp
    }

    pub const fn day_count(&self) -> DayCount {
        self.day_count
    }

    pub const fn start(&self) -> InterestStart {
        self.start
    }

    pub const fn rounding(&self) -> Rounding {
        self.rounding
    }

    pub const fn grace_if_paid_in_full(&self) -> bool {
        self.grace_if_paid_in_full
    }

    /// True when no interest should be charged: the card has a grace period and the
    /// previous statement balance was fully paid by its due date.
    pub fn grace_applies(&self, previous_statement_paid_in_full: bool) -> bool {
        self.grace_if_paid_in_full && previous_statement_paid_in_full
    }

    /// Interest accrued over `from..=to`, starting from `opening` (the balance at the
    /// start of `from`, excluding `entries`).
    pub fn accrue(
        &self,
        opening: Money,
        from: Date,
        to: Date,
        entries: &[Entry],
    ) -> Result<Money, InterestError> {
        if to < from {
            return Err(InterestError::InvalidWindow);
        }
        let currency = opening.currency();
        let mut dated: Vec<(Date, i64)> = Vec::with_capacity(entries.len());
        for e in entries {
            check_currency(currency, e.amount)?;
            if e.amount.minor() <= 0 {
                return Err(InterestError::NonPositiveEntry);
            }
            let (date, signed) = match e.kind {
                EntryKind::Charge => (self.effective_date(e), e.amount.minor()),
                EntryKind::Credit => (e.posting_date, -e.amount.minor()),
            };
            if date <= to {
                dated.push((date, signed));
            }
        }
        dated.sort_by_key(|(d, _)| *d);

        let first = dated.first().map_or(from, |(d, _)| (*d).min(from));
        let mut numerator: i128 = 0;
        let mut balance: i128 = 0;
        let mut next = 0;
        let mut day = first;
        loop {
            if day == from {
                balance += i128::from(opening.minor());
            }
            while next < dated.len() && dated[next].0 == day {
                balance += i128::from(dated[next].1);
                next += 1;
            }
            if balance > 0 {
                numerator += self
                    .day_count
                    .day_numerator(balance, self.annual_rate_bp, day);
            }
            if day == to {
                break;
            }
            day += Duration::DAY;
        }

        let minor = self
            .rounding
            .apply(numerator, DENOMINATOR)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, currency))
    }

    fn effective_date(&self, e: &Entry) -> Date {
        match self.start {
            InterestStart::TransactionDate => e.transaction_date,
            InterestStart::PostingDate => e.posting_date,
        }
    }
}

fn check_currency(expected: Currency, m: Money) -> Result<(), MoneyError> {
    if m.currency() == expected {
        Ok(())
    } else {
        Err(MoneyError::CurrencyMismatch {
            left: expected,
            right: m.currency(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Increases the balance (purchase, cash advance, fee).
    Charge,
    /// Decreases the balance (payment, refund).
    Credit,
}

/// A balance change. `amount` is always positive; `kind` gives the direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub transaction_date: Date,
    pub posting_date: Date,
    pub amount: Money,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rounding::RoundingMode;
    use time::macros::date;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    fn rules(bp: u32, dc: DayCount, start: InterestStart) -> InterestRules {
        InterestRules::new(bp, dc, start, Rounding::default(), true).unwrap()
    }

    fn sixteen() -> InterestRules {
        rules(1_600, DayCount::Actual365, InterestStart::TransactionDate)
    }

    fn charge(tx: Date, posted: Date, baht: i64) -> Entry {
        Entry {
            kind: EntryKind::Charge,
            transaction_date: tx,
            posting_date: posted,
            amount: thb(baht),
        }
    }

    fn payment(on: Date, baht: i64) -> Entry {
        Entry {
            kind: EntryKind::Credit,
            transaction_date: on,
            posting_date: on,
            amount: thb(baht),
        }
    }

    #[test]
    fn flat_balance_for_thirty_days() {
        // ฿10,000 × 16% × 30/365 = ฿131.5068… → ฿131.51
        let got = sixteen().accrue(
            thb(10_000),
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 30),
            &[],
        );
        assert_eq!(got, Ok(Money::new(13_151, Currency::Thb)));
    }

    #[test]
    fn payment_stops_interest_from_its_posting_day() {
        // ฿10,000 for Nov 1–10 (10 days), then ฿0. ฿10,000 × 0.16 × 10/365 = ฿43.835… → ฿43.84
        let got = sixteen().accrue(
            thb(10_000),
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 30),
            &[payment(date!(2026 - 11 - 11), 10_000)],
        );
        assert_eq!(got, Ok(Money::new(4_384, Currency::Thb)));
    }

    #[test]
    fn charge_counts_from_transaction_or_posting_date() {
        // ฿3,650 charged Nov 21, posted Nov 23; window ends Nov 30.
        // Transaction date: 10 days × ฿1.60/day = ฿16.00. Posting date: 8 days = ฿12.80.
        let e = [charge(date!(2026 - 11 - 21), date!(2026 - 11 - 23), 3_650)];
        let from = date!(2026 - 11 - 01);
        let to = date!(2026 - 11 - 30);
        let by_tx = sixteen().accrue(thb(0), from, to, &e);
        let by_post = rules(1_600, DayCount::Actual365, InterestStart::PostingDate).accrue(
            thb(0),
            from,
            to,
            &e,
        );
        assert_eq!(by_tx, Ok(thb(16)));
        assert_eq!(by_post, Ok(Money::new(1_280, Currency::Thb)));
    }

    #[test]
    fn charge_made_before_window_accrues_from_transaction_date() {
        // ฿3,650 made Oct 30, posted Nov 2 (after an Oct 31 cut-off). Window Nov 1–10.
        // Accrues Oct 30 – Nov 10 = 12 days × ฿1.60 = ฿19.20.
        let got = sixteen().accrue(
            thb(0),
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 10),
            &[charge(date!(2026 - 10 - 30), date!(2026 - 11 - 02), 3_650)],
        );
        assert_eq!(got, Ok(Money::new(1_920, Currency::Thb)));
    }

    #[test]
    fn credit_balance_earns_nothing() {
        let got = sixteen().accrue(thb(-500), date!(2026 - 11 - 01), date!(2026 - 11 - 30), &[]);
        assert_eq!(got, Ok(thb(0)));
    }

    #[test]
    fn day_count_methods() {
        // ฿36,600 at 10% for 1 day in a leap year.
        let day = date!(2028 - 03 - 01);
        let calc = |dc| {
            rules(1_000, dc, InterestStart::TransactionDate).accrue(thb(36_600), day, day, &[])
        };
        assert_eq!(calc(DayCount::ActualActual), Ok(thb(10))); // /366
        assert_eq!(
            calc(DayCount::Actual365),
            Ok(Money::new(1_003, Currency::Thb))
        ); // 10.027 → 10.03
        assert_eq!(
            calc(DayCount::Actual360),
            Ok(Money::new(1_017, Currency::Thb))
        ); // 10.166 → 10.17
    }

    #[test]
    fn uses_the_configured_rounding() {
        let r = InterestRules::new(
            1_600,
            DayCount::Actual365,
            InterestStart::TransactionDate,
            Rounding::new(100, RoundingMode::Down).unwrap(),
            true,
        )
        .unwrap();
        let got = r.accrue(
            thb(10_000),
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 30),
            &[],
        );
        assert_eq!(got, Ok(thb(131)));
    }

    #[test]
    fn grace_needs_both_setting_and_full_payment() {
        let with = sixteen();
        let without = InterestRules::new(
            1_600,
            DayCount::Actual365,
            InterestStart::TransactionDate,
            Rounding::default(),
            false,
        )
        .unwrap();
        assert!(with.grace_applies(true));
        assert!(!with.grace_applies(false));
        assert!(!without.grace_applies(true));
    }

    #[test]
    fn zero_rate_card() {
        let r = rules(0, DayCount::Actual365, InterestStart::TransactionDate);
        assert_eq!(
            r.accrue(
                thb(10_000),
                date!(2026 - 11 - 01),
                date!(2026 - 11 - 30),
                &[]
            ),
            Ok(thb(0))
        );
    }

    #[test]
    fn rejects_bad_settings_and_inputs() {
        assert_eq!(
            InterestRules::new(
                10_001,
                DayCount::Actual365,
                InterestStart::TransactionDate,
                Rounding::default(),
                true
            ),
            Err(InterestError::InvalidRate(10_001))
        );
        let r = sixteen();
        assert_eq!(
            r.accrue(thb(0), date!(2026 - 11 - 02), date!(2026 - 11 - 01), &[]),
            Err(InterestError::InvalidWindow)
        );
        let zero = Entry {
            amount: thb(0),
            ..payment(date!(2026 - 11 - 01), 1)
        };
        assert_eq!(
            r.accrue(
                thb(0),
                date!(2026 - 11 - 01),
                date!(2026 - 11 - 02),
                &[zero]
            ),
            Err(InterestError::NonPositiveEntry)
        );
        let usd = Entry {
            amount: Money::new(100, Currency::Usd),
            ..payment(date!(2026 - 11 - 01), 1)
        };
        assert!(matches!(
            r.accrue(thb(0), date!(2026 - 11 - 01), date!(2026 - 11 - 02), &[usd]),
            Err(InterestError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }
}
