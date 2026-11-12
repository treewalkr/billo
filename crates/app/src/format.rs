//! User-facing formatting. Integer arithmetic only.

use billo_domain::{Currency, Money};
use time::{Date, Month};

use crate::strings::{Lang, lang};

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

const TH_MONTHS_SHORT: [&str; 12] = [
    "ม.ค.",
    "ก.พ.",
    "มี.ค.",
    "เม.ย.",
    "พ.ค.",
    "มิ.ย.",
    "ก.ค.",
    "ส.ค.",
    "ก.ย.",
    "ต.ค.",
    "พ.ย.",
    "ธ.ค.",
];

const TH_MONTHS: [&str; 12] = [
    "มกราคม",
    "กุมภาพันธ์",
    "มีนาคม",
    "เมษายน",
    "พฤษภาคม",
    "มิถุนายน",
    "กรกฎาคม",
    "สิงหาคม",
    "กันยายน",
    "ตุลาคม",
    "พฤศจิกายน",
    "ธันวาคม",
];

/// Buddhist Era year (พ.ศ.): Gregorian + 543.
pub const fn buddhist_year(year: i32) -> i32 {
    year + 543
}

fn month_index(month: Month) -> usize {
    usize::from(u8::from(month)) - 1
}

/// `25 Nov 2026` in English, `25 พ.ย. 2569` (Buddhist Era) in Thai.
pub fn date_in(d: Date, lang: Lang) -> String {
    match lang {
        Lang::En => {
            let month = d.month().to_string();
            format!("{} {} {}", d.day(), &month[..3], d.year())
        }
        Lang::Th => format!(
            "{} {} {}",
            d.day(),
            TH_MONTHS_SHORT[month_index(d.month())],
            buddhist_year(d.year())
        ),
    }
}

/// [`date_in`] the current language.
pub fn date(d: Date) -> String {
    date_in(d, lang())
}

/// `November 2026` in English, `พฤศจิกายน 2569` in Thai.
pub fn month_year_in(year: i32, month: Month, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{month} {year}"),
        Lang::Th => format!("{} {}", TH_MONTHS[month_index(month)], buddhist_year(year)),
    }
}

/// [`month_year_in`] the current language.
pub fn month_year(year: i32, month: Month) -> String {
    month_year_in(year, month, lang())
}

/// Basis points as a whole percentage, rounded down: `6_666` → `"66%"`.
pub fn whole_percent(bp: u32) -> String {
    format!("{}%", bp / 100)
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

    #[test]
    fn buddhist_era_dates() {
        assert_eq!(buddhist_year(2026), 2569);
        assert_eq!(date_in(date!(2026 - 11 - 16), Lang::Th), "16 พ.ย. 2569");
        assert_eq!(date_in(date!(2027 - 01 - 01), Lang::Th), "1 ม.ค. 2570");
        assert_eq!(date_in(date!(2024 - 02 - 29), Lang::Th), "29 ก.พ. 2567");
        assert_eq!(date_in(date!(2026 - 12 - 31), Lang::Th), "31 ธ.ค. 2569");
        assert_eq!(date_in(date!(2026 - 11 - 16), Lang::En), "16 Nov 2026");
    }

    #[test]
    fn every_thai_month() {
        let short: Vec<_> = (1..=12)
            .map(|m| {
                let d = Date::from_calendar_date(2026, Month::try_from(m).unwrap(), 1).unwrap();
                date_in(d, Lang::Th)
            })
            .collect();
        assert_eq!(
            short,
            [
                "1 ม.ค. 2569",
                "1 ก.พ. 2569",
                "1 มี.ค. 2569",
                "1 เม.ย. 2569",
                "1 พ.ค. 2569",
                "1 มิ.ย. 2569",
                "1 ก.ค. 2569",
                "1 ส.ค. 2569",
                "1 ก.ย. 2569",
                "1 ต.ค. 2569",
                "1 พ.ย. 2569",
                "1 ธ.ค. 2569",
            ]
        );
    }

    #[test]
    fn month_titles() {
        assert_eq!(
            month_year_in(2026, Month::November, Lang::En),
            "November 2026"
        );
        assert_eq!(
            month_year_in(2026, Month::November, Lang::Th),
            "พฤศจิกายน 2569"
        );
        assert_eq!(month_year_in(2025, Month::January, Lang::Th), "มกราคม 2568");
    }

    #[test]
    fn whole_percents_round_down() {
        assert_eq!(whole_percent(6_666), "66%");
        assert_eq!(whole_percent(12_500), "125%");
        assert_eq!(whole_percent(0), "0%");
    }
}
