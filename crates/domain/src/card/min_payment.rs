//! Minimum payment on a card statement. Every number here is a user setting.
//!
//! ```text
//! base    = max(percent × revolving, floor), never more than revolving
//! minimum = base + due_in_full, never more than the whole statement balance
//! ```
//!
//! - `revolving`: the part of the statement balance the percentage applies to.
//! - `due_in_full`: amounts the bank always wants in full this month
//!   (installment portions, overdue minimums, fees). Banks differ on what goes here,
//!   so the caller decides.

use std::fmt;

use crate::money::{Money, MoneyError};
use crate::rounding::Rounding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinPaymentError {
    /// Percent must be 0.01%..=100% (1..=10_000 basis points).
    InvalidPercent(u32),
    NegativeFloor,
    NegativeDueInFull,
    Money(MoneyError),
}

impl From<MoneyError> for MinPaymentError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for MinPaymentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPercent(bp) => {
                write!(
                    f,
                    "minimum payment percent must be 1..=10000 basis points, got {bp}"
                )
            }
            Self::NegativeFloor => f.write_str("minimum payment floor cannot be negative"),
            Self::NegativeDueInFull => f.write_str("amount due in full cannot be negative"),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for MinPaymentError {}

/// A card's minimum payment setting, e.g. "8% or ฿1,000, whichever is higher".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinPaymentRule {
    percent_bp: u32,
    floor: Money,
    rounding: Rounding,
}

impl MinPaymentRule {
    /// `percent_bp` in basis points: 800 = 8%.
    pub fn new(percent_bp: u32, floor: Money, rounding: Rounding) -> Result<Self, MinPaymentError> {
        if !(1..=10_000).contains(&percent_bp) {
            return Err(MinPaymentError::InvalidPercent(percent_bp));
        }
        if floor.is_negative() {
            return Err(MinPaymentError::NegativeFloor);
        }
        Ok(Self {
            percent_bp,
            floor,
            rounding,
        })
    }

    pub const fn percent_bp(&self) -> u32 {
        self.percent_bp
    }

    pub const fn floor(&self) -> Money {
        self.floor
    }

    pub const fn rounding(&self) -> Rounding {
        self.rounding
    }

    pub fn minimum_due(
        &self,
        revolving: Money,
        due_in_full: Money,
    ) -> Result<Money, MinPaymentError> {
        let currency = revolving.currency();
        // Validates matching currencies for all three amounts.
        let statement_total = revolving.checked_add(due_in_full)?;
        self.floor.checked_add(revolving)?;
        if due_in_full.is_negative() {
            return Err(MinPaymentError::NegativeDueInFull);
        }

        let base = if revolving.minor() <= 0 {
            0
        } else {
            let pct = self
                .rounding
                .apply(
                    i128::from(revolving.minor()) * i128::from(self.percent_bp),
                    10_000,
                )
                .ok_or(MoneyError::Overflow)?;
            pct.max(self.floor.minor()).min(revolving.minor())
        };

        let minimum = base
            .checked_add(due_in_full.minor())
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(
            minimum.min(statement_total.minor().max(0)),
            currency,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use crate::rounding::RoundingMode;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    fn rule(bp: u32, floor_baht: i64) -> MinPaymentRule {
        MinPaymentRule::new(bp, thb(floor_baht), Rounding::default()).unwrap()
    }

    #[test]
    fn percent_wins_above_floor() {
        // 8% of ฿20,000 = ฿1,600 > ฿1,000 floor
        assert_eq!(
            rule(800, 1_000).minimum_due(thb(20_000), thb(0)),
            Ok(thb(1_600))
        );
    }

    #[test]
    fn floor_wins_on_small_balance() {
        // 8% of ฿5,000 = ฿400 < ฿1,000 floor
        assert_eq!(
            rule(800, 1_000).minimum_due(thb(5_000), thb(0)),
            Ok(thb(1_000))
        );
    }

    #[test]
    fn balance_below_floor_is_paid_in_full() {
        assert_eq!(rule(800, 1_000).minimum_due(thb(600), thb(0)), Ok(thb(600)));
    }

    #[test]
    fn due_in_full_is_added_on_top() {
        // ฿1,600 + ฿2,500 installment portion
        assert_eq!(
            rule(800, 1_000).minimum_due(thb(20_000), thb(2_500)),
            Ok(thb(4_100))
        );
    }

    #[test]
    fn credit_balance_offsets_and_never_goes_negative() {
        assert_eq!(rule(800, 1_000).minimum_due(thb(-300), thb(0)), Ok(thb(0)));
        // ฿300 credit against a ฿2,500 installment portion → ฿2,200
        assert_eq!(
            rule(800, 1_000).minimum_due(thb(-300), thb(2_500)),
            Ok(thb(2_200))
        );
    }

    #[test]
    fn uses_the_configured_rounding() {
        // 10% of ฿12,345.67 = ฿1,234.567
        let revolving = Money::new(1_234_567, Currency::Thb);
        let satang = MinPaymentRule::new(1_000, thb(0), Rounding::default()).unwrap();
        let baht_up =
            MinPaymentRule::new(1_000, thb(0), Rounding::new(100, RoundingMode::Up).unwrap())
                .unwrap();
        assert_eq!(
            satang.minimum_due(revolving, thb(0)),
            Ok(Money::new(123_457, Currency::Thb))
        );
        assert_eq!(baht_up.minimum_due(revolving, thb(0)), Ok(thb(1_235)));
    }

    #[test]
    fn rejects_bad_settings_and_inputs() {
        let r = Rounding::default();
        assert_eq!(
            MinPaymentRule::new(0, thb(0), r),
            Err(MinPaymentError::InvalidPercent(0))
        );
        assert_eq!(
            MinPaymentRule::new(10_001, thb(0), r),
            Err(MinPaymentError::InvalidPercent(10_001))
        );
        assert_eq!(
            MinPaymentRule::new(800, thb(-1), r),
            Err(MinPaymentError::NegativeFloor)
        );
        assert_eq!(
            rule(800, 1_000).minimum_due(thb(5_000), thb(-1)),
            Err(MinPaymentError::NegativeDueInFull)
        );
    }

    #[test]
    fn rejects_mixed_currencies() {
        let usd = Money::new(100, Currency::Usd);
        assert!(matches!(
            rule(800, 1_000).minimum_due(usd, Money::zero(Currency::Usd)),
            Err(MinPaymentError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }
}
