//! Builds one card statement from the card's settings and its entries.
//!
//! ```text
//! opening   = previous statement balance
//! closing   = opening + charges − credits + interest   (entries posted in the period)
//! paid by due = credits posted after the previous statement date, up to its due date
//! interest  = 0 if grace applies, else daily accrual over the period
//! overdue   = previous minimum − paid by due (never below 0)
//! minimum   = min-payment rule on (closing − due_in_full − overdue),
//!             plus due_in_full and overdue
//! ```
//!
//! Not modelled yet: trailing interest on the previous cycle's purchases when grace is
//! lost, and late fees (add them as charges).

use std::fmt;

use time::{Date, Month};

use crate::calendar::BusinessCalendar;
use crate::card::cycle::{CycleRules, StatementPeriod};
use crate::card::interest::{Entry, EntryKind, InterestError, InterestRules};
use crate::card::min_payment::{MinPaymentError, MinPaymentRule};
use crate::money::{Money, MoneyError};

/// All of a card's user settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardRules {
    pub cycle: CycleRules,
    pub min_payment: MinPaymentRule,
    pub interest: InterestRules,
}

/// What the next statement needs to know about the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviousStatement {
    pub statement_date: Date,
    pub due_date: Date,
    pub balance: Money,
    pub minimum_due: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Statement {
    pub period: StatementPeriod,
    pub opening_balance: Money,
    pub charges: Money,
    pub credits: Money,
    pub interest: Money,
    pub closing_balance: Money,
    /// Previous minimum not paid by its due date.
    pub overdue: Money,
    pub minimum_due: Money,
    /// Whether the previous statement was paid in full by its due date.
    pub previous_paid_in_full: bool,
}

impl Statement {
    /// Input for building the following month's statement.
    pub fn as_previous(&self) -> PreviousStatement {
        PreviousStatement {
            statement_date: self.period.statement_date,
            due_date: self.period.due_date,
            balance: self.closing_balance,
            minimum_due: self.minimum_due,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementError {
    Interest(InterestError),
    MinPayment(MinPaymentError),
    Money(MoneyError),
}

impl From<InterestError> for StatementError {
    fn from(e: InterestError) -> Self {
        Self::Interest(e)
    }
}

impl From<MinPaymentError> for StatementError {
    fn from(e: MinPaymentError) -> Self {
        Self::MinPayment(e)
    }
}

impl From<MoneyError> for StatementError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for StatementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interest(e) => e.fmt(f),
            Self::MinPayment(e) => e.fmt(f),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for StatementError {}

impl CardRules {
    /// The statement whose cut-off falls in `year`/`month`.
    ///
    /// - `previous`: `None` for the card's first statement.
    /// - `entries`: all of the card's entries; only those posted in the period are used.
    /// - `due_in_full`: amounts due in full this month (installment portions); its
    ///   currency is the card's currency.
    pub fn statement(
        &self,
        year: i32,
        month: Month,
        previous: Option<&PreviousStatement>,
        entries: &[Entry],
        due_in_full: Money,
        cal: &impl BusinessCalendar,
    ) -> Result<Statement, StatementError> {
        let currency = due_in_full.currency();
        let zero = Money::zero(currency);
        let period = self.cycle.period_for_month(year, month, cal);
        let opening = previous.map_or(zero, |p| p.balance);

        let in_period: Vec<Entry> = entries
            .iter()
            .copied()
            .filter(|e| period.contains(e.posting_date))
            .collect();

        let mut charges = zero;
        let mut credits = zero;
        for e in &in_period {
            match e.kind {
                EntryKind::Charge => charges = charges.checked_add(e.amount)?,
                EntryKind::Credit => credits = credits.checked_add(e.amount)?,
            }
        }

        let (paid_in_full, overdue) = match previous {
            None => (true, zero),
            Some(p) => {
                let mut paid = zero;
                for e in entries {
                    if e.kind == EntryKind::Credit
                        && e.posting_date > p.statement_date
                        && e.posting_date <= p.due_date
                    {
                        paid = paid.checked_add(e.amount)?;
                    }
                }
                let overdue = p.minimum_due.checked_sub(paid)?;
                (
                    paid.minor() >= p.balance.minor(),
                    Money::new(overdue.minor().max(0), currency),
                )
            }
        };

        let interest = if self.interest.grace_applies(paid_in_full) {
            zero
        } else {
            self.interest
                .accrue(opening, period.start, period.statement_date, &in_period)?
        };

        let closing = opening
            .checked_add(charges)?
            .checked_sub(credits)?
            .checked_add(interest)?;
        let always_due = due_in_full.checked_add(overdue)?;
        let revolving = closing.checked_sub(always_due)?;
        let minimum_due = self.min_payment.minimum_due(revolving, always_due)?;

        Ok(Statement {
            period,
            opening_balance: opening,
            charges,
            credits,
            interest,
            closing_balance: closing,
            overdue,
            minimum_due,
            previous_paid_in_full: paid_in_full,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::EveryDay;
    use crate::card::cycle::{DueRule, DueShift};
    use crate::card::interest::{DayCount, InterestStart};
    use crate::money::Currency;
    use crate::rounding::Rounding;
    use time::macros::date;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    const fn satang(minor: i64) -> Money {
        Money::new(minor, Currency::Thb)
    }

    /// Cut-off 25th, due 20 days later, 8% or ฿1,000, 16% Actual/365 from transaction date.
    fn card(grace: bool) -> CardRules {
        CardRules {
            cycle: CycleRules::new(25, DueRule::DaysAfterStatement(20), DueShift::None).unwrap(),
            min_payment: MinPaymentRule::new(800, thb(1_000), Rounding::default()).unwrap(),
            interest: InterestRules::new(
                1_600,
                DayCount::Actual365,
                InterestStart::TransactionDate,
                Rounding::default(),
                grace,
            )
            .unwrap(),
        }
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

    /// Oct 25 statement: ฿36,500, minimum ฿2,920, due Nov 14.
    fn october() -> PreviousStatement {
        PreviousStatement {
            statement_date: date!(2026 - 10 - 25),
            due_date: date!(2026 - 11 - 14),
            balance: thb(36_500),
            minimum_due: thb(2_920),
        }
    }

    fn november(prev: Option<&PreviousStatement>, entries: &[Entry], grace: bool) -> Statement {
        card(grace)
            .statement(2026, Month::November, prev, entries, thb(0), &EveryDay)
            .unwrap()
    }

    #[test]
    fn first_statement_has_no_interest() {
        let s = november(
            None,
            &[charge(date!(2026 - 11 - 01), date!(2026 - 11 - 02), 5_000)],
            true,
        );
        assert_eq!(s.period.start, date!(2026 - 10 - 26));
        assert_eq!(s.period.due_date, date!(2026 - 12 - 15));
        assert_eq!(s.closing_balance, thb(5_000));
        assert_eq!(s.interest, thb(0));
        assert_eq!(s.minimum_due, thb(1_000)); // floor beats 8% of ฿5,000
    }

    #[test]
    fn partial_payment_charges_interest() {
        // ฿36,500 Oct 26–Nov 9 (15 days × ฿16 = ฿240)
        // ฿33,500 Nov 10–25 (16 days = ฿234.9589…)
        // ฿3,650 charge Nov 20–25 (6 days × ฿1.60 = ฿9.60) → ฿484.5589… → ฿484.56
        let entries = [
            payment(date!(2026 - 11 - 10), 3_000),
            charge(date!(2026 - 11 - 20), date!(2026 - 11 - 21), 3_650),
        ];
        let s = november(Some(&october()), &entries, true);
        assert!(!s.previous_paid_in_full);
        assert_eq!(s.interest, satang(48_456));
        assert_eq!(s.closing_balance, satang(3_763_456)); // 36,500 − 3,000 + 3,650 + 484.56
        assert_eq!(s.overdue, thb(0)); // ฿3,000 ≥ ฿2,920 minimum
        assert_eq!(s.minimum_due, satang(301_076)); // 8% of ฿37,634.56
    }

    #[test]
    fn unpaid_minimum_becomes_overdue_and_is_due_in_full() {
        // Paid ฿1,000 of the ฿2,920 minimum → ฿1,920 overdue.
        // Interest: ฿240 + ฿35,500 × 16 days (฿248.9863…) + ฿9.60 = ฿498.5863… → ฿498.59
        let entries = [
            payment(date!(2026 - 11 - 10), 1_000),
            charge(date!(2026 - 11 - 20), date!(2026 - 11 - 21), 3_650),
        ];
        let s = november(Some(&october()), &entries, true);
        assert_eq!(s.interest, satang(49_859));
        assert_eq!(s.closing_balance, satang(3_964_859));
        assert_eq!(s.overdue, thb(1_920));
        // 8% of (39,648.59 − 1,920 = 37,728.59) = 3,018.2872 → 3,018.29, + 1,920
        assert_eq!(s.minimum_due, satang(493_829));
    }

    #[test]
    fn paid_in_full_on_due_date_keeps_grace() {
        let prev = PreviousStatement {
            balance: thb(5_000),
            minimum_due: thb(1_000),
            ..october()
        };
        let entries = [
            charge(date!(2026 - 11 - 01), date!(2026 - 11 - 02), 2_000),
            payment(date!(2026 - 11 - 14), 5_000),
        ];
        let s = november(Some(&prev), &entries, true);
        assert!(s.previous_paid_in_full);
        assert_eq!(s.interest, thb(0));
        assert_eq!(s.closing_balance, thb(2_000));
        assert_eq!(s.minimum_due, thb(1_000));
    }

    #[test]
    fn paying_a_day_late_loses_grace() {
        // Daily balances: ฿5,000 × 6 days, ฿7,000 × 14, ฿2,000 × 11 = 150,000 baht-days
        // × 0.16 / 365 = ฿65.7534… → ฿65.75
        let prev = PreviousStatement {
            balance: thb(5_000),
            minimum_due: thb(1_000),
            ..october()
        };
        let entries = [
            charge(date!(2026 - 11 - 01), date!(2026 - 11 - 02), 2_000),
            payment(date!(2026 - 11 - 15), 5_000),
        ];
        let s = november(Some(&prev), &entries, true);
        assert!(!s.previous_paid_in_full);
        assert_eq!(s.interest, satang(6_575));
        assert_eq!(s.overdue, thb(1_000));
    }

    #[test]
    fn card_without_grace_always_charges_interest() {
        let prev = PreviousStatement {
            balance: thb(5_000),
            minimum_due: thb(1_000),
            ..october()
        };
        let s = november(Some(&prev), &[payment(date!(2026 - 11 - 14), 5_000)], false);
        // ฿5,000 Oct 26–Nov 13 (19 days) × 0.16 / 365 = ฿41.6438… → ฿41.64
        assert_eq!(s.interest, satang(4_164));
    }

    #[test]
    fn ignores_entries_posted_outside_the_period() {
        let entries = [
            charge(date!(2026 - 10 - 20), date!(2026 - 10 - 25), 9_999), // previous statement
            charge(date!(2026 - 11 - 25), date!(2026 - 11 - 26), 9_999), // next statement
            charge(date!(2026 - 11 - 25), date!(2026 - 11 - 25), 1_000),
        ];
        let s = november(None, &entries, true);
        assert_eq!(s.charges, thb(1_000));
    }

    #[test]
    fn installment_portion_is_due_in_full() {
        use crate::card::installment::{InstallmentPlan, Remainder};
        // ฿12,000 over 10 months posting on the 5th, plus a ฿20,000 purchase.
        let plan = InstallmentPlan::new(
            thb(12_000),
            10,
            0,
            date!(2026 - 11 - 05),
            Rounding::default(),
            Remainder::Last,
        )
        .unwrap();
        let rules = card(true);
        let period = rules
            .cycle
            .period_for_month(2026, Month::November, &EveryDay);
        let mut entries = plan.entries();
        entries.push(charge(date!(2026 - 11 - 01), date!(2026 - 11 - 02), 20_000));
        let s = rules
            .statement(
                2026,
                Month::November,
                None,
                &entries,
                plan.due_in_period(&period),
                &EveryDay,
            )
            .unwrap();
        assert_eq!(s.closing_balance, thb(21_200));
        // 8% of the ฿20,000 revolving part = ฿1,600, plus the ฿1,200 portion in full
        assert_eq!(s.minimum_due, thb(2_800));
    }

    #[test]
    fn chains_into_the_next_month() {
        let cal = EveryDay;
        let rules = card(true);
        let entries = [
            charge(date!(2026 - 11 - 01), date!(2026 - 11 - 02), 5_000),
            payment(date!(2026 - 12 - 15), 5_000),
        ];
        let nov = rules
            .statement(2026, Month::November, None, &entries, thb(0), &cal)
            .unwrap();
        let dec = rules
            .statement(
                2026,
                Month::December,
                Some(&nov.as_previous()),
                &entries,
                thb(0),
                &cal,
            )
            .unwrap();
        assert_eq!(dec.opening_balance, thb(5_000));
        assert!(dec.previous_paid_in_full); // paid on the Dec 15 due date
        assert_eq!(dec.closing_balance, thb(0));
        assert_eq!(dec.minimum_due, thb(0));
    }
}
