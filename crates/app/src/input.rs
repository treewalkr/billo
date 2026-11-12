//! Parsing what users type into forms. Integer arithmetic only: `"1,234.56"` becomes
//! `123456` minor units without ever touching a float.

use std::fmt;

use billo_domain::Currency;
use time::{Date, Month};

use crate::strings as s;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    Required,
    NotANumber,
    TooManyDecimals(u32),
    Negative,
    TooLarge,
    OutOfRange { min: i64, max: i64 },
    NotADate,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required => f.write_str(s::required()),
            Self::NotANumber => f.write_str(s::not_a_number()),
            Self::TooManyDecimals(0) => f.write_str(s::whole_numbers_only()),
            Self::TooManyDecimals(n) => f.write_str(&s::at_most_decimals(*n)),
            Self::Negative => f.write_str(s::cant_be_negative()),
            Self::TooLarge => f.write_str(s::too_large()),
            Self::OutOfRange { min, max } => f.write_str(&s::between(*min, *max)),
            Self::NotADate => f.write_str(s::not_a_date()),
        }
    }
}

/// A decimal with at most `scale` fractional digits, as an integer scaled by
/// `10^scale`: `parse_decimal("1,234.5", 2) == Ok(123450)`. Commas, spaces and
/// underscores between digits are ignored; a leading `-` is allowed only with
/// `allow_negative`.
pub fn parse_decimal(text: &str, scale: u32, allow_negative: bool) -> Result<i64, InputError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(InputError::Required);
    }
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if negative && !allow_negative {
        return Err(InputError::Negative);
    }
    let cleaned: String = t
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '_'))
        .collect();
    let (whole, fraction) = match cleaned.split_once('.') {
        Some((w, f)) => (w, f),
        None => (cleaned.as_str(), ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err(InputError::NotANumber);
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err(InputError::NotANumber);
    }
    // Trailing zeros beyond the scale are harmless ("1.500" baht).
    let fraction = fraction.trim_end_matches('0');
    let digits = u32::try_from(fraction.len()).map_err(|_| InputError::TooLarge)?;
    if digits > scale {
        return Err(InputError::TooManyDecimals(scale));
    }

    let mut value: i64 = 0;
    for c in whole.chars().chain(fraction.chars()) {
        let d = i64::from(c.to_digit(10).ok_or(InputError::NotANumber)?);
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(d))
            .ok_or(InputError::TooLarge)?;
    }
    for _ in digits..scale {
        value = value.checked_mul(10).ok_or(InputError::TooLarge)?;
    }
    Ok(if negative { -value } else { value })
}

/// An amount in `currency`'s minor units: `"1,234.56"` THB → `123456`.
pub fn parse_amount(
    text: &str,
    currency: Currency,
    allow_negative: bool,
) -> Result<i64, InputError> {
    parse_decimal(text, currency.minor_units(), allow_negative)
}

/// A percentage in basis points: `"16"` → `1600`, `"0.69"` → `69`.
pub fn parse_percent_bp(text: &str) -> Result<u32, InputError> {
    let bp = parse_decimal(text, 2, false)?;
    u32::try_from(bp).map_err(|_| InputError::TooLarge)
}

/// A whole number within `min..=max`.
pub fn parse_int(text: &str, min: i64, max: i64) -> Result<i64, InputError> {
    let v = parse_decimal(text, 0, min < 0)?;
    if v < min || v > max {
        return Err(InputError::OutOfRange { min, max });
    }
    Ok(v)
}

/// `"YYYY-MM-DD"`, as produced by `<input type="date">`.
pub fn parse_date(text: &str) -> Result<Date, InputError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(InputError::Required);
    }
    let mut parts = t.splitn(3, '-');
    let mut next = |len: usize| -> Result<i64, InputError> {
        let p = parts.next().ok_or(InputError::NotADate)?;
        if p.len() != len || !p.chars().all(|c| c.is_ascii_digit()) {
            return Err(InputError::NotADate);
        }
        p.parse().map_err(|_| InputError::NotADate)
    };
    let (y, m, d) = (next(4)?, next(2)?, next(2)?);
    let month = u8::try_from(m)
        .ok()
        .and_then(|m| Month::try_from(m).ok())
        .ok_or(InputError::NotADate)?;
    let year = i32::try_from(y).map_err(|_| InputError::NotADate)?;
    let day = u8::try_from(d).map_err(|_| InputError::NotADate)?;
    Date::from_calendar_date(year, month, day).map_err(|_| InputError::NotADate)
}

/// Blank means `None`; anything else must parse.
pub fn optional<T>(
    text: &str,
    parse: impl FnOnce(&str) -> Result<T, InputError>,
) -> Result<Option<T>, InputError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        parse(text).map(Some)
    }
}

/// Labels an error with the form field it came from.
pub fn field<T>(label: &str, r: Result<T, InputError>) -> Result<T, String> {
    r.map_err(|e| format!("{label}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn decimals_become_minor_units() {
        assert_eq!(parse_decimal("1,234.56", 2, false), Ok(123_456));
        assert_eq!(parse_decimal("1234.5", 2, false), Ok(123_450));
        assert_eq!(parse_decimal("1234", 2, false), Ok(123_400));
        assert_eq!(parse_decimal(" 0.05 ", 2, false), Ok(5));
        assert_eq!(parse_decimal(".5", 2, false), Ok(50));
        assert_eq!(parse_decimal("5.", 2, false), Ok(500));
        assert_eq!(parse_decimal("1 000 000", 2, false), Ok(100_000_000));
        assert_eq!(parse_decimal("+7", 2, false), Ok(700));
        assert_eq!(parse_decimal("1.500", 2, false), Ok(150));
        assert_eq!(parse_decimal("0", 2, false), Ok(0));
    }

    #[test]
    fn rejects_bad_numbers() {
        assert_eq!(parse_decimal("", 2, false), Err(InputError::Required));
        assert_eq!(parse_decimal("  ", 2, false), Err(InputError::Required));
        assert_eq!(parse_decimal("abc", 2, false), Err(InputError::NotANumber));
        assert_eq!(
            parse_decimal("1.2.3", 2, false),
            Err(InputError::NotANumber)
        );
        assert_eq!(parse_decimal(".", 2, false), Err(InputError::NotANumber));
        assert_eq!(parse_decimal("1e3", 2, false), Err(InputError::NotANumber));
        assert_eq!(parse_decimal("฿100", 2, false), Err(InputError::NotANumber));
        assert_eq!(
            parse_decimal("1.234", 2, false),
            Err(InputError::TooManyDecimals(2))
        );
        assert_eq!(
            parse_decimal("99999999999999999999", 2, false),
            Err(InputError::TooLarge)
        );
        assert_eq!(
            parse_decimal("92233720368547758.08", 2, false),
            Err(InputError::TooLarge)
        );
        assert_eq!(
            parse_decimal("92233720368547758.07", 2, false),
            Ok(i64::MAX)
        );
    }

    #[test]
    fn negatives_only_when_allowed() {
        assert_eq!(parse_decimal("-12.5", 2, false), Err(InputError::Negative));
        assert_eq!(parse_decimal("-12.5", 2, true), Ok(-1_250));
        assert_eq!(parse_decimal("- 1,000", 2, true), Ok(-100_000));
    }

    #[test]
    fn amounts_follow_currency_decimals() {
        assert_eq!(parse_amount("1,234.56", Currency::Thb, false), Ok(123_456));
        assert_eq!(parse_amount("1500", Currency::Jpy, false), Ok(1_500));
        assert_eq!(
            parse_amount("1500.5", Currency::Jpy, false),
            Err(InputError::TooManyDecimals(0))
        );
    }

    #[test]
    fn percents_become_basis_points() {
        assert_eq!(parse_percent_bp("16"), Ok(1_600));
        assert_eq!(parse_percent_bp("0.69"), Ok(69));
        assert_eq!(parse_percent_bp("8.5"), Ok(850));
        assert_eq!(parse_percent_bp("0"), Ok(0));
        assert_eq!(parse_percent_bp("-1"), Err(InputError::Negative));
        assert_eq!(
            parse_percent_bp("0.125"),
            Err(InputError::TooManyDecimals(2))
        );
    }

    #[test]
    fn ints_in_range() {
        assert_eq!(parse_int("25", 1, 31), Ok(25));
        assert_eq!(
            parse_int("32", 1, 31),
            Err(InputError::OutOfRange { min: 1, max: 31 })
        );
        assert_eq!(parse_int("2.5", 1, 31), Err(InputError::TooManyDecimals(0)));
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("2026-11-12"), Ok(date!(2026 - 11 - 12)));
        assert_eq!(parse_date("2028-02-29"), Ok(date!(2028 - 02 - 29)));
        assert_eq!(parse_date("2026-02-29"), Err(InputError::NotADate));
        assert_eq!(parse_date("2026-13-01"), Err(InputError::NotADate));
        assert_eq!(parse_date("2026-1-01"), Err(InputError::NotADate));
        assert_eq!(parse_date("12/11/2026"), Err(InputError::NotADate));
        assert_eq!(parse_date(""), Err(InputError::Required));
    }

    #[test]
    fn optional_and_field_labels() {
        assert_eq!(optional("", parse_date), Ok(None));
        assert_eq!(
            optional("2026-11-12", parse_date),
            Ok(Some(date!(2026 - 11 - 12)))
        );
        assert_eq!(
            field("Amount", parse_decimal("x", 2, false)),
            Err::<i64, _>("Amount: not a number".to_owned())
        );
    }
}
