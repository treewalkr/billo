//! User-facing formatting. Integer arithmetic only.

use billo_domain::{Currency, Money};
use time::Date;

const fn symbol(currency: Currency) -> &'static str {
    match currency {
        Currency::Thb => "฿",
        Currency::Usd => "$",
        Currency::Eur => "€",
        Currency::Jpy => "¥",
    }
}

/// `฿1,234.56`, `-฿0.05`, `¥500`.
pub fn money(m: Money) -> String {
    let digits = m.currency().minor_units();
    let scale = 10_u64.pow(digits);
    let abs = m.minor().unsigned_abs();
    let (whole, fraction) = (abs / scale, abs % scale);

    let whole = whole.to_string();
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }

    let sign = if m.is_negative() { "-" } else { "" };
    let sym = symbol(m.currency());
    if digits == 0 {
        format!("{sign}{sym}{grouped}")
    } else {
        let width = digits as usize;
        format!("{sign}{sym}{grouped}.{fraction:0width$}")
    }
}

/// A scaled integer as plain digits for editing: `plain(123456, 2) == "1234.56"`.
pub fn plain(value: i64, scale: u32) -> String {
    if scale == 0 {
        return value.to_string();
    }
    let factor = 10_u64.pow(scale);
    let abs = value.unsigned_abs();
    let sign = if value < 0 { "-" } else { "" };
    let width = scale as usize;
    format!("{sign}{}.{:0width$}", abs / factor, abs % factor)
}

/// Minor units in `currency` as plain digits: `"1234.56"`, `"1500"` (JPY).
pub fn plain_amount(minor: i64, currency: Currency) -> String {
    plain(minor, currency.minor_units())
}

/// Basis points as a percentage without trailing zeros: `1600` → `"16"`, `69` → `"0.69"`.
pub fn percent(bp: u32) -> String {
    let s = plain(i64::from(bp), 2);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `25 Nov 2026`.
pub fn date(d: Date) -> String {
    let month = d.month().to_string();
    format!("{} {} {}", d.day(), &month[..3], d.year())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn thb(minor: i64) -> String {
        money(Money::new(minor, Currency::Thb))
    }

    #[test]
    fn zero() {
        assert_eq!(thb(0), "฿0.00");
    }

    #[test]
    fn pads_satang() {
        assert_eq!(thb(5), "฿0.05");
        assert_eq!(thb(10), "฿0.10");
        assert_eq!(thb(100), "฿1.00");
        assert_eq!(thb(12_345), "฿123.45");
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(thb(99_999), "฿999.99");
        assert_eq!(thb(100_000), "฿1,000.00");
        assert_eq!(thb(123_456), "฿1,234.56");
        assert_eq!(thb(12_345_678_901), "฿123,456,789.01");
        assert_eq!(thb(100_000_000_000), "฿1,000,000,000.00");
    }

    #[test]
    fn negative() {
        assert_eq!(thb(-5), "-฿0.05");
        assert_eq!(thb(-123_456), "-฿1,234.56");
    }

    #[test]
    fn extremes_do_not_overflow() {
        assert_eq!(thb(i64::MIN), "-฿92,233,720,368,547,758.08");
        assert_eq!(thb(i64::MAX), "฿92,233,720,368,547,758.07");
    }

    #[test]
    fn zero_decimal_currency() {
        assert_eq!(money(Money::new(1_500, Currency::Jpy)), "¥1,500");
    }

    #[test]
    fn plain_numbers() {
        assert_eq!(plain(123_456, 2), "1234.56");
        assert_eq!(plain(5, 2), "0.05");
        assert_eq!(plain(-5, 2), "-0.05");
        assert_eq!(plain(1_500, 0), "1500");
        assert_eq!(plain_amount(100_000, Currency::Thb), "1000.00");
    }

    #[test]
    fn percents() {
        assert_eq!(percent(1_600), "16");
        assert_eq!(percent(69), "0.69");
        assert_eq!(percent(850), "8.5");
        assert_eq!(percent(0), "0");
    }

    #[test]
    fn dates() {
        assert_eq!(date(date!(2026 - 11 - 25)), "25 Nov 2026");
        assert_eq!(date(date!(2026 - 12 - 1)), "1 Dec 2026");
    }
}
