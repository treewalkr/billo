//! API DTOs (`billo-api`) → domain types (`billo-domain`). The one place this mapping
//! lives in the app; the domain constructors do the validation.
//!
//! Errors are plain sentences naming the setting, shown next to the item they belong
//! to so one bad record never breaks a whole screen.

use std::fmt;

use billo_api::rules::{self, CardSettings, LoanSettings, RoundingSettings};
use billo_domain::card::{
    CardRules, CreditLine, CycleRules, DueRule, DueShift, InterestRules, InterestStart,
    MinPaymentRule,
};
use billo_domain::{Currency, DayCount, LoanMethod, LoanTerms, Money, Rounding, RoundingMode};

/// Why a stored record can't be turned into domain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

fn invalid(what: &str, e: impl fmt::Display) -> Invalid {
    Invalid(format!("{what}: {e}"))
}

pub fn currency(code: &str) -> Result<Currency, Invalid> {
    Currency::from_code(code).ok_or_else(|| Invalid(format!("unsupported currency {code}")))
}

pub fn rounding(what: &str, r: RoundingSettings) -> Result<Rounding, Invalid> {
    let mode = match r.mode {
        rules::RoundingMode::Up => RoundingMode::Up,
        rules::RoundingMode::Down => RoundingMode::Down,
        rules::RoundingMode::HalfUp => RoundingMode::HalfUp,
    };
    Rounding::new(r.step, mode).map_err(|e| invalid(what, e))
}

pub fn day_count(d: rules::DayCount) -> DayCount {
    match d {
        rules::DayCount::Actual365 => DayCount::Actual365,
        rules::DayCount::ActualActual => DayCount::ActualActual,
        rules::DayCount::Actual360 => DayCount::Actual360,
    }
}

/// A card's settings as domain rules, plus its credit line when a limit is set.
pub fn card_rules(
    s: &CardSettings,
    currency: Currency,
) -> Result<(CardRules, Option<CreditLine>), Invalid> {
    let due = match s.due {
        rules::DueRule::DaysAfterStatement(d) => DueRule::DaysAfterStatement(d),
        rules::DueRule::DayOfMonth(d) => DueRule::DayOfMonth(d),
    };
    let shift = match s.due_shift {
        rules::DueShift::None => DueShift::None,
        rules::DueShift::Next => DueShift::Next,
        rules::DueShift::Previous => DueShift::Previous,
    };
    let cycle =
        CycleRules::new(s.statement_day, due, shift).map_err(|e| invalid("Statement cycle", e))?;
    let min_payment = MinPaymentRule::new(
        s.min_payment.percent_bp,
        Money::new(s.min_payment.floor_minor, currency),
        rounding("Minimum payment rounding", s.min_payment.rounding)?,
    )
    .map_err(|e| invalid("Minimum payment", e))?;
    let start = match s.interest.start {
        rules::InterestStart::TransactionDate => InterestStart::TransactionDate,
        rules::InterestStart::PostingDate => InterestStart::PostingDate,
    };
    let interest = InterestRules::new(
        s.interest.annual_rate_bp,
        day_count(s.interest.day_count),
        start,
        rounding("Interest rounding", s.interest.rounding)?,
        s.interest.grace_if_paid_in_full,
    )
    .map_err(|e| invalid("Interest", e))?;
    let limit = s
        .credit_limit_minor
        .map(|l| CreditLine::new(Money::new(l, currency)))
        .transpose()
        .map_err(|e| invalid("Credit limit", e))?;
    Ok((
        CardRules {
            cycle,
            min_payment,
            interest,
        },
        limit,
    ))
}

pub fn loan_terms(s: &LoanSettings, currency: Currency) -> Result<LoanTerms, Invalid> {
    let method = match s.method {
        rules::LoanMethod::Effective => LoanMethod::Effective(day_count(s.day_count)),
        rules::LoanMethod::Flat => LoanMethod::Flat,
    };
    if s.installment_minor.is_some_and(|m| m <= 0) {
        return Err(Invalid("Monthly installment: must be positive".into()));
    }
    LoanTerms::new(billo_domain::LoanSettings {
        principal: Money::new(s.principal_minor, currency),
        annual_rate_bp: s.annual_rate_bp,
        months: s.months,
        start: s.start,
        first_payment: s.first_payment,
        method,
        rounding: rounding("Loan rounding", s.rounding)?,
        installment: s.installment_minor.map(|m| Money::new(m, currency)),
    })
    .map_err(|e| invalid("Loan", e))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use billo_api::rules::{
        InterestSettings, LoanMethod as ApiLoanMethod, MinPaymentSettings, RoundingMode as Mode,
    };
    use time::macros::date;

    pub fn card_settings() -> CardSettings {
        CardSettings {
            statement_day: 25,
            due: rules::DueRule::DaysAfterStatement(20),
            due_shift: rules::DueShift::Next,
            credit_limit_minor: Some(5_000_000),
            min_payment: MinPaymentSettings {
                percent_bp: 800,
                floor_minor: 100_000,
                rounding: RoundingSettings::default(),
            },
            interest: InterestSettings {
                annual_rate_bp: 1_600,
                day_count: rules::DayCount::Actual365,
                start: rules::InterestStart::TransactionDate,
                rounding: RoundingSettings::default(),
                grace_if_paid_in_full: true,
            },
        }
    }

    #[test]
    fn card_settings_map_to_rules() {
        let (r, limit) = card_rules(&card_settings(), Currency::Thb).unwrap();
        assert_eq!(r.cycle.statement_day(), 25);
        assert_eq!(r.cycle.due(), DueRule::DaysAfterStatement(20));
        assert_eq!(r.cycle.due_shift(), DueShift::Next);
        assert_eq!(r.min_payment.percent_bp(), 800);
        assert_eq!(r.min_payment.floor(), Money::new(100_000, Currency::Thb));
        assert_eq!(r.interest.annual_rate_bp(), 1_600);
        assert_eq!(r.interest.start(), InterestStart::TransactionDate);
        assert!(r.interest.grace_if_paid_in_full());
        assert_eq!(limit.unwrap().limit(), Money::new(5_000_000, Currency::Thb));
    }

    #[test]
    fn bad_card_settings_name_the_setting() {
        let mut s = card_settings();
        s.statement_day = 0;
        assert!(
            card_rules(&s, Currency::Thb)
                .unwrap_err()
                .0
                .starts_with("Statement cycle")
        );

        let mut s = card_settings();
        s.min_payment.rounding = RoundingSettings {
            step: 0,
            mode: Mode::Up,
        };
        assert!(
            card_rules(&s, Currency::Thb)
                .unwrap_err()
                .0
                .starts_with("Minimum payment rounding")
        );
    }

    #[test]
    fn loan_settings_map_to_terms() {
        let s = LoanSettings {
            principal_minor: 10_000_000,
            annual_rate_bp: 1_500,
            months: 12,
            start: date!(2026 - 01 - 01),
            first_payment: date!(2026 - 02 - 01),
            method: ApiLoanMethod::Flat,
            day_count: rules::DayCount::Actual365,
            rounding: RoundingSettings::default(),
            installment_minor: None,
        };
        let t = loan_terms(&s, Currency::Thb).unwrap();
        assert_eq!(t.months(), 12);
        assert_eq!(t.method(), LoanMethod::Flat);
        assert_eq!(t.principal(), Money::new(10_000_000, Currency::Thb));

        let bad = LoanSettings {
            installment_minor: Some(0),
            ..s
        };
        assert!(loan_terms(&bad, Currency::Thb).is_err());
    }

    #[test]
    fn currencies() {
        assert_eq!(currency("THB"), Ok(Currency::Thb));
        assert!(currency("XYZ").is_err());
    }
}
