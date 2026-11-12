//! Money as integer minor units (e.g. satang) tagged with a currency.
//!
//! Floats are never used. Amounts in different currencies cannot be combined.

use std::fmt;

/// ISO 4217 currency. Add variants as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    Thb,
    Usd,
    Eur,
    Jpy,
}

impl Currency {
    /// ISO 4217 alphabetic code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Thb => "THB",
            Self::Usd => "USD",
            Self::Eur => "EUR",
            Self::Jpy => "JPY",
        }
    }

    /// Number of decimal places in the minor unit (THB: 2 → satang).
    pub const fn minor_units(self) -> u32 {
        match self {
            Self::Jpy => 0,
            Self::Thb | Self::Usd | Self::Eur => 2,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    CurrencyMismatch { left: Currency, right: Currency },
    Overflow,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrencyMismatch { left, right } => {
                write!(f, "cannot combine {left} with {right}")
            }
            Self::Overflow => f.write_str("money amount overflow"),
        }
    }
}

impl std::error::Error for MoneyError {}

/// An amount in minor units of a currency. `Money::new(12_345, Currency::Thb)` is ฿123.45.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

impl Money {
    pub const fn new(minor: i64, currency: Currency) -> Self {
        Self { minor, currency }
    }

    pub const fn zero(currency: Currency) -> Self {
        Self::new(0, currency)
    }

    pub const fn minor(self) -> i64 {
        self.minor
    }

    pub const fn currency(self) -> Currency {
        self.currency
    }

    pub const fn is_zero(self) -> bool {
        self.minor == 0
    }

    pub const fn is_negative(self) -> bool {
        self.minor < 0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        let minor = self
            .minor
            .checked_add(other.minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self::new(minor, self.currency))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        let minor = self
            .minor
            .checked_sub(other.minor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self::new(minor, self.currency))
    }

    pub fn checked_neg(self) -> Result<Self, MoneyError> {
        let minor = self.minor.checked_neg().ok_or(MoneyError::Overflow)?;
        Ok(Self::new(minor, self.currency))
    }

    fn same_currency(self, other: Self) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

/// Plain, locale-free form: `THB 1234.56`, `JPY 500`, `THB -0.05`.
/// User-facing formatting (฿, thousands separators, Thai digits) belongs in the app.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let units = self.currency.minor_units();
        let sign = if self.minor < 0 { "-" } else { "" };
        let abs = self.minor.unsigned_abs();
        if units == 0 {
            return write!(f, "{} {sign}{abs}", self.currency);
        }
        let scale = 10_u64.pow(units);
        let width = units as usize;
        write!(
            f,
            "{} {sign}{}.{:0width$}",
            self.currency,
            abs / scale,
            abs % scale
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn thb(minor: i64) -> Money {
        Money::new(minor, Currency::Thb)
    }

    #[test]
    fn adds_and_subtracts_same_currency() {
        assert_eq!(thb(150).checked_add(thb(250)), Ok(thb(400)));
        assert_eq!(thb(150).checked_sub(thb(250)), Ok(thb(-100)));
    }

    #[test]
    fn rejects_mixed_currencies() {
        let usd = Money::new(100, Currency::Usd);
        assert_eq!(
            thb(100).checked_add(usd),
            Err(MoneyError::CurrencyMismatch {
                left: Currency::Thb,
                right: Currency::Usd
            })
        );
    }

    #[test]
    fn reports_overflow_instead_of_wrapping() {
        assert_eq!(thb(i64::MAX).checked_add(thb(1)), Err(MoneyError::Overflow));
        assert_eq!(thb(i64::MIN).checked_neg(), Err(MoneyError::Overflow));
    }

    #[test]
    fn displays_minor_units() {
        assert_eq!(thb(123_456).to_string(), "THB 1234.56");
        assert_eq!(thb(5).to_string(), "THB 0.05");
        assert_eq!(thb(-5).to_string(), "THB -0.05");
        assert_eq!(Money::new(500, Currency::Jpy).to_string(), "JPY 500");
        assert_eq!(thb(i64::MIN).to_string(), "THB -92233720368547758.08");
    }

    #[test]
    fn zero_and_sign_helpers() {
        assert!(Money::zero(Currency::Thb).is_zero());
        assert!(thb(-1).is_negative());
        assert!(!thb(0).is_negative());
    }
}
