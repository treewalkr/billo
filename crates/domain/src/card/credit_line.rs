//! Credit limits and available credit.
//!
//! A cash card / card loan (บัตรกดเงินสด) is a [`CardRules`](super::CardRules) with the
//! grace period turned off: interest runs daily from each withdrawal until it is repaid.

use std::fmt;

use crate::money::{Money, MoneyError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditLineError {
    NegativeLimit,
    Money(MoneyError),
}

impl From<MoneyError> for CreditLineError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for CreditLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeLimit => f.write_str("credit limit cannot be negative"),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CreditLineError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditLine {
    limit: Money,
}

impl CreditLine {
    pub fn new(limit: Money) -> Result<Self, CreditLineError> {
        if limit.is_negative() {
            return Err(CreditLineError::NegativeLimit);
        }
        Ok(Self { limit })
    }

    pub fn limit(&self) -> Money {
        self.limit
    }

    /// Credit still available.
    ///
    /// - `balance`: current outstanding balance (negative = credit in the card's favour).
    /// - `held`: amounts that use the limit but are not in the balance yet, e.g. the
    ///   unposted part of installment plans ([`remaining_after`]) or pending authorisations.
    ///
    /// Can be negative when the card is over its limit.
    ///
    /// [`remaining_after`]: super::InstallmentPlan::remaining_after
    pub fn available(&self, balance: Money, held: Money) -> Result<Money, CreditLineError> {
        Ok(self.limit.checked_sub(balance)?.checked_sub(held)?)
    }

    /// Share of the limit in use, in basis points (10_000 = 100%). `None` for a zero limit.
    pub fn utilisation_bp(
        &self,
        balance: Money,
        held: Money,
    ) -> Result<Option<u32>, CreditLineError> {
        let used = balance.checked_add(held)?;
        self.limit.checked_sub(used)?;
        if self.limit.is_zero() {
            return Ok(None);
        }
        let bp = i128::from(used.minor().max(0)) * 10_000 / i128::from(self.limit.minor());
        Ok(Some(u32::try_from(bp).unwrap_or(u32::MAX)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::EveryDay;
    use crate::card::{
        CardRules, CycleRules, DayCount, DueRule, DueShift, Entry, EntryKind, InterestRules,
        InterestStart, MinPaymentRule,
    };
    use crate::money::Currency;
    use crate::rounding::Rounding;
    use time::Month;
    use time::macros::date;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    #[test]
    fn available_credit_counts_balance_and_holds() {
        let line = CreditLine::new(thb(50_000)).unwrap();
        assert_eq!(line.available(thb(12_000), thb(9_600)), Ok(thb(28_400)));
        assert_eq!(line.available(thb(-500), thb(0)), Ok(thb(50_500)));
        assert_eq!(line.available(thb(51_000), thb(0)), Ok(thb(-1_000)));
    }

    #[test]
    fn utilisation() {
        let line = CreditLine::new(thb(50_000)).unwrap();
        assert_eq!(line.utilisation_bp(thb(12_500), thb(0)), Ok(Some(2_500)));
        assert_eq!(line.utilisation_bp(thb(-100), thb(0)), Ok(Some(0)));
        assert_eq!(
            CreditLine::new(thb(0))
                .unwrap()
                .utilisation_bp(thb(0), thb(0)),
            Ok(None)
        );
    }

    #[test]
    fn rejects_negative_limit() {
        assert_eq!(
            CreditLine::new(thb(-1)),
            Err(CreditLineError::NegativeLimit)
        );
    }

    #[test]
    fn cash_card_charges_interest_from_the_withdrawal_day() {
        // 25% a year, no grace. ฿36,500 withdrawn Nov 16, cut-off Nov 25:
        // 10 days × ฿25/day = ฿250.
        let cash_card = CardRules {
            cycle: CycleRules::new(25, DueRule::DaysAfterStatement(20), DueShift::None).unwrap(),
            min_payment: MinPaymentRule::new(300, thb(500), Rounding::default()).unwrap(),
            interest: InterestRules::new(
                2_500,
                DayCount::Actual365,
                InterestStart::TransactionDate,
                Rounding::default(),
                false,
            )
            .unwrap(),
        };
        let withdrawal = Entry {
            kind: EntryKind::Charge,
            transaction_date: date!(2026 - 11 - 16),
            posting_date: date!(2026 - 11 - 16),
            amount: thb(36_500),
        };
        let s = cash_card
            .statement(
                2026,
                Month::November,
                None,
                &[withdrawal],
                thb(0),
                &EveryDay,
            )
            .unwrap();
        assert_eq!(s.interest, thb(250));
        assert_eq!(s.closing_balance, thb(36_750));
        // 3% of ฿36,750 = ฿1,102.50
        assert_eq!(s.minimum_due, Money::new(110_250, Currency::Thb));
    }
}
