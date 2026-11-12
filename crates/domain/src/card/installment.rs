//! Installment plans on a card (ผ่อน 0% or with interest). Every number is a user setting.
//!
//! - Each month one portion posts to the card as a charge and is due in full on that
//!   statement (feed [`InstallmentPlan::due_in_period`] into `due_in_full`).
//! - Interest, if any, is flat: `principal × monthly rate × months`, spread over portions.
//! - Portions are rounded with the plan's [`Rounding`]; one portion (first or last)
//!   absorbs the remainder so the portions always sum to the exact total.

use std::fmt;

use time::Date;

use crate::calendar::{add_months, clamped_date};
use crate::card::cycle::StatementPeriod;
use crate::card::interest::{Entry, EntryKind};
use crate::money::{Money, MoneyError};
use crate::rounding::Rounding;

/// Which portion absorbs the rounding remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Remainder {
    First,
    #[default]
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallmentError {
    ZeroMonths,
    NonPositivePrincipal,
    /// Flat monthly rate must be 0..=10_000 basis points.
    InvalidRate(u32),
    Money(MoneyError),
}

impl From<MoneyError> for InstallmentError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for InstallmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMonths => f.write_str("an installment plan needs at least 1 month"),
            Self::NonPositivePrincipal => f.write_str("installment principal must be positive"),
            Self::InvalidRate(bp) => {
                write!(f, "monthly rate must be 0..=10000 basis points, got {bp}")
            }
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for InstallmentError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Portion {
    /// 1-based.
    pub number: u16,
    pub posting_date: Date,
    pub principal: Money,
    pub interest: Money,
}

impl Portion {
    pub fn amount(&self) -> Money {
        // Same currency by construction; cannot overflow (sums to a valid total).
        Money::new(
            self.principal.minor() + self.interest.minor(),
            self.principal.currency(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallmentPlan {
    principal: Money,
    months: u16,
    monthly_rate_bp: u32,
    first_posting: Date,
    rounding: Rounding,
    remainder: Remainder,
    schedule: Vec<Portion>,
}

impl InstallmentPlan {
    /// - `monthly_rate_bp`: flat rate per month in basis points (0 for ผ่อน 0%; 69 = 0.69%).
    /// - `first_posting`: the date the first portion posts; later portions post on the
    ///   same day of following months (clamped to month end).
    pub fn new(
        principal: Money,
        months: u16,
        monthly_rate_bp: u32,
        first_posting: Date,
        rounding: Rounding,
        remainder: Remainder,
    ) -> Result<Self, InstallmentError> {
        if months == 0 {
            return Err(InstallmentError::ZeroMonths);
        }
        if principal.minor() <= 0 {
            return Err(InstallmentError::NonPositivePrincipal);
        }
        if monthly_rate_bp > 10_000 {
            return Err(InstallmentError::InvalidRate(monthly_rate_bp));
        }
        let interest_total = rounding
            .apply(
                i128::from(principal.minor()) * i128::from(monthly_rate_bp) * i128::from(months),
                10_000,
            )
            .ok_or(MoneyError::Overflow)?;
        let principals = split(principal.minor(), months, rounding, remainder)?;
        let interests = split(interest_total, months, rounding, remainder)?;
        let currency = principal.currency();
        let schedule = (0..months)
            .map(|i| {
                let (y, m) = add_months(first_posting.year(), first_posting.month(), i32::from(i));
                Portion {
                    number: i + 1,
                    posting_date: clamped_date(y, m, first_posting.day()),
                    principal: Money::new(principals[usize::from(i)], currency),
                    interest: Money::new(interests[usize::from(i)], currency),
                }
            })
            .collect();
        Ok(Self {
            principal,
            months,
            monthly_rate_bp,
            first_posting,
            rounding,
            remainder,
            schedule,
        })
    }

    pub fn principal(&self) -> Money {
        self.principal
    }

    pub fn months(&self) -> u16 {
        self.months
    }

    pub fn monthly_rate_bp(&self) -> u32 {
        self.monthly_rate_bp
    }

    pub fn first_posting(&self) -> Date {
        self.first_posting
    }

    pub fn rounding(&self) -> Rounding {
        self.rounding
    }

    pub fn remainder(&self) -> Remainder {
        self.remainder
    }

    pub fn schedule(&self) -> &[Portion] {
        &self.schedule
    }

    /// Total of all portions (principal + interest).
    pub fn total(&self) -> Money {
        self.sum(|_| true)
    }

    /// Portions posted in the statement period: add to that statement's `due_in_full`.
    pub fn due_in_period(&self, period: &StatementPeriod) -> Money {
        self.sum(|p| period.contains(p.posting_date))
    }

    /// Everything not yet posted as of `date` (portions posting after `date`).
    pub fn remaining_after(&self, date: Date) -> Money {
        self.sum(|p| p.posting_date > date)
    }

    /// Principal not yet posted as of `date`: the usual early-close amount when the bank
    /// waives future interest.
    pub fn remaining_principal_after(&self, date: Date) -> Money {
        let minor = self
            .schedule
            .iter()
            .filter(|p| p.posting_date > date)
            .map(|p| p.principal.minor())
            .sum();
        Money::new(minor, self.principal.currency())
    }

    /// Card charges for each portion (transaction and posting date = portion date).
    pub fn entries(&self) -> Vec<Entry> {
        self.schedule
            .iter()
            .map(|p| Entry {
                kind: EntryKind::Charge,
                transaction_date: p.posting_date,
                posting_date: p.posting_date,
                amount: p.amount(),
            })
            .collect()
    }

    fn sum(&self, keep: impl Fn(&Portion) -> bool) -> Money {
        let minor = self
            .schedule
            .iter()
            .filter(|p| keep(p))
            .map(|p| p.amount().minor())
            .sum();
        Money::new(minor, self.principal.currency())
    }
}

/// Split `total` into `n` rounded parts that sum exactly to `total`.
fn split(
    total: i64,
    n: u16,
    rounding: Rounding,
    remainder: Remainder,
) -> Result<Vec<i64>, MoneyError> {
    let part = rounding
        .apply(i128::from(total), i128::from(n))
        .ok_or(MoneyError::Overflow)?;
    let mut parts = vec![part; usize::from(n)];
    let rest = total - part * (i64::from(n) - 1);
    let idx = match remainder {
        Remainder::First => 0,
        Remainder::Last => usize::from(n) - 1,
    };
    parts[idx] = rest;
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use crate::rounding::RoundingMode;
    use time::macros::date;

    const fn satang(minor: i64) -> Money {
        Money::new(minor, Currency::Thb)
    }

    const fn thb(baht: i64) -> Money {
        satang(baht * 100)
    }

    fn zero_pct(baht: i64, months: u16) -> InstallmentPlan {
        InstallmentPlan::new(
            thb(baht),
            months,
            0,
            date!(2026 - 11 - 05),
            Rounding::default(),
            Remainder::Last,
        )
        .unwrap()
    }

    #[test]
    fn even_split() {
        let p = zero_pct(12_000, 10);
        assert!(p.schedule().iter().all(|x| x.amount() == thb(1_200)));
        assert_eq!(p.total(), thb(12_000));
    }

    #[test]
    fn remainder_goes_to_last_or_first_and_sums_exactly() {
        // ฿10,000 / 3 = ฿3,333.33 × 2 + ฿3,333.34
        let last = zero_pct(10_000, 3);
        let amounts: Vec<_> = last.schedule().iter().map(Portion::amount).collect();
        assert_eq!(amounts, [satang(333_333), satang(333_333), satang(333_334)]);
        let first = InstallmentPlan::new(
            thb(10_000),
            3,
            0,
            date!(2026 - 11 - 05),
            Rounding::default(),
            Remainder::First,
        )
        .unwrap();
        assert_eq!(first.schedule()[0].amount(), satang(333_334));
        assert_eq!(first.total(), thb(10_000));
    }

    #[test]
    fn whole_baht_rounding_down() {
        // ฿10,000 / 3 rounded down to the baht: ฿3,333 × 2 + ฿3,334
        let p = InstallmentPlan::new(
            thb(10_000),
            3,
            0,
            date!(2026 - 11 - 05),
            Rounding::new(100, RoundingMode::Down).unwrap(),
            Remainder::Last,
        )
        .unwrap();
        let amounts: Vec<_> = p.schedule().iter().map(Portion::amount).collect();
        assert_eq!(amounts, [thb(3_333), thb(3_333), thb(3_334)]);
    }

    #[test]
    fn flat_interest_is_spread_over_portions() {
        // ฿12,000 × 0.69% × 12 = ฿993.60 interest → ฿82.80 a month; ฿1,000 principal a month
        let p = InstallmentPlan::new(
            thb(12_000),
            12,
            69,
            date!(2026 - 11 - 05),
            Rounding::default(),
            Remainder::Last,
        )
        .unwrap();
        assert_eq!(p.schedule()[0].interest, satang(8_280));
        assert_eq!(p.schedule()[0].amount(), satang(108_280));
        assert_eq!(p.total(), satang(1_299_360));
    }

    #[test]
    fn posting_dates_clamp_and_roll_over_years() {
        let p = InstallmentPlan::new(
            thb(3_000),
            3,
            0,
            date!(2026 - 12 - 31),
            Rounding::default(),
            Remainder::Last,
        )
        .unwrap();
        let dates: Vec<_> = p.schedule().iter().map(|x| x.posting_date).collect();
        assert_eq!(
            dates,
            [
                date!(2026 - 12 - 31),
                date!(2027 - 01 - 31),
                date!(2027 - 02 - 28)
            ]
        );
    }

    #[test]
    fn due_in_period_and_remaining() {
        let p = zero_pct(12_000, 10); // posts on the 5th, Nov 2026 – Aug 2027
        let period = StatementPeriod {
            start: date!(2026 - 11 - 26),
            statement_date: date!(2026 - 12 - 25),
            due_date: date!(2027 - 01 - 14),
        };
        assert_eq!(p.due_in_period(&period), thb(1_200)); // the Dec 5 portion
        assert_eq!(p.remaining_after(date!(2026 - 12 - 05)), thb(9_600)); // 8 left
        assert_eq!(
            p.remaining_principal_after(date!(2026 - 12 - 05)),
            thb(9_600)
        );
        assert_eq!(p.entries().len(), 10);
    }

    #[test]
    fn early_close_waives_future_interest() {
        let p = InstallmentPlan::new(
            thb(12_000),
            12,
            69,
            date!(2026 - 11 - 05),
            Rounding::default(),
            Remainder::Last,
        )
        .unwrap();
        // After 2 portions: 10 × ฿1,000 principal left; 10 × ฿1,082.80 with interest.
        assert_eq!(
            p.remaining_principal_after(date!(2026 - 12 - 05)),
            thb(10_000)
        );
        assert_eq!(p.remaining_after(date!(2026 - 12 - 05)), satang(1_082_800));
    }

    #[test]
    fn rejects_bad_settings() {
        let d = date!(2026 - 11 - 05);
        let r = Rounding::default();
        assert_eq!(
            InstallmentPlan::new(thb(1), 0, 0, d, r, Remainder::Last),
            Err(InstallmentError::ZeroMonths)
        );
        assert_eq!(
            InstallmentPlan::new(thb(0), 3, 0, d, r, Remainder::Last),
            Err(InstallmentError::NonPositivePrincipal)
        );
        assert_eq!(
            InstallmentPlan::new(thb(1), 3, 10_001, d, r, Remainder::Last),
            Err(InstallmentError::InvalidRate(10_001))
        );
    }
}
