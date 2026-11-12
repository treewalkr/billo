//! Input checks shared by the resources. Settings are validated by converting the wire
//! types (`billo_api::rules`) into `billo-domain` types, whose constructors enforce
//! the rules. Error messages name fields, never values.

use billo_api::rules::{
    AccountRules, CardSettings, DayCount, LoanMethod, LoanSettings, RULES_VERSION, RoundingMode,
    RoundingSettings,
};
use billo_api::{AccountKind, Money};
use billo_domain::card::{
    CardRules, CreditLine, CycleRules, DueRule, DueShift, InterestRules, InterestStart,
    MinPaymentRule,
};
use billo_domain::{Currency, LoanTerms, Rounding};

use crate::error::{ApiError, ApiResult};

pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_NOTE_CHARS: usize = 2_000;

pub fn currency(field: &str, code: &str) -> ApiResult<Currency> {
    Currency::from_code(code)
        .ok_or_else(|| ApiError::validation(format!("{field}: unknown currency code")))
}

/// Trims `name`; it must be non-empty and at most [`MAX_NAME_CHARS`].
pub fn name(field: &str, name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::validation(format!("{field}: required")));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::validation(format!("{field}: too long")));
    }
    Ok(name.to_owned())
}

/// Like [`name`], but blank means `None`.
pub fn optional_name(field: &str, value: Option<&str>) -> ApiResult<Option<String>> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => name(field, v).map(Some),
    }
}

/// A money value in a known currency; with `positive`, it must be above zero.
pub fn money(field: &str, m: &Money, positive: bool) -> ApiResult<billo_domain::Money> {
    let c = currency(&format!("{field}.currency"), &m.currency)?;
    if positive && m.minor <= 0 {
        return Err(ApiError::validation(format!("{field}: must be positive")));
    }
    Ok(billo_domain::Money::new(m.minor, c))
}

pub fn rounding(field: &str, r: RoundingSettings) -> ApiResult<Rounding> {
    let mode = match r.mode {
        RoundingMode::Up => billo_domain::RoundingMode::Up,
        RoundingMode::Down => billo_domain::RoundingMode::Down,
        RoundingMode::HalfUp => billo_domain::RoundingMode::HalfUp,
    };
    Rounding::new(r.step, mode).map_err(|e| ApiError::validation(format!("{field}: {e}")))
}

pub fn day_count(d: DayCount) -> billo_domain::DayCount {
    match d {
        DayCount::Actual365 => billo_domain::DayCount::Actual365,
        DayCount::ActualActual => billo_domain::DayCount::ActualActual,
        DayCount::Actual360 => billo_domain::DayCount::Actual360,
    }
}

/// Checks `rules` for an account of `kind` in `currency`: the shape version, which
/// settings the kind may have, and the settings themselves (through the domain types).
pub fn account_rules(kind: AccountKind, currency: Currency, rules: &AccountRules) -> ApiResult<()> {
    if rules.v != RULES_VERSION {
        return Err(ApiError::validation(format!(
            "rules.v: unsupported version (expected {RULES_VERSION})"
        )));
    }
    let card_ok = matches!(kind, AccountKind::Card | AccountKind::Revolving);
    match &rules.card {
        Some(_) if !card_ok => {
            return Err(ApiError::validation(
                "rules.card: only for card and revolving accounts",
            ));
        }
        Some(card) => {
            card_rules(card, currency)?;
        }
        None => {}
    }
    match &rules.loan {
        Some(_) if kind != AccountKind::Loan => {
            return Err(ApiError::validation("rules.loan: only for loan accounts"));
        }
        Some(loan) => {
            loan_terms(loan, currency)?;
        }
        None => {}
    }
    Ok(())
}

pub fn card_rules(
    s: &CardSettings,
    currency: Currency,
) -> ApiResult<(CardRules, Option<CreditLine>)> {
    let due = match s.due {
        billo_api::rules::DueRule::DaysAfterStatement(d) => DueRule::DaysAfterStatement(d),
        billo_api::rules::DueRule::DayOfMonth(d) => DueRule::DayOfMonth(d),
    };
    let shift = match s.due_shift {
        billo_api::rules::DueShift::None => DueShift::None,
        billo_api::rules::DueShift::Next => DueShift::Next,
        billo_api::rules::DueShift::Previous => DueShift::Previous,
    };
    let cycle = CycleRules::new(s.statement_day, due, shift)
        .map_err(|e| ApiError::validation(format!("rules.card: {e}")))?;
    let min_payment = MinPaymentRule::new(
        s.min_payment.percent_bp,
        billo_domain::Money::new(s.min_payment.floor_minor, currency),
        rounding("rules.card.min_payment.rounding", s.min_payment.rounding)?,
    )
    .map_err(|e| ApiError::validation(format!("rules.card.min_payment: {e}")))?;
    let start = match s.interest.start {
        billo_api::rules::InterestStart::TransactionDate => InterestStart::TransactionDate,
        billo_api::rules::InterestStart::PostingDate => InterestStart::PostingDate,
    };
    let interest = InterestRules::new(
        s.interest.annual_rate_bp,
        day_count(s.interest.day_count),
        start,
        rounding("rules.card.interest.rounding", s.interest.rounding)?,
        s.interest.grace_if_paid_in_full,
    )
    .map_err(|e| ApiError::validation(format!("rules.card.interest: {e}")))?;
    let limit = s
        .credit_limit_minor
        .map(|l| CreditLine::new(billo_domain::Money::new(l, currency)))
        .transpose()
        .map_err(|e| ApiError::validation(format!("rules.card.credit_limit_minor: {e}")))?;
    Ok((
        CardRules {
            cycle,
            min_payment,
            interest,
        },
        limit,
    ))
}

pub fn loan_terms(s: &LoanSettings, currency: Currency) -> ApiResult<LoanTerms> {
    let method = match s.method {
        LoanMethod::Effective => billo_domain::LoanMethod::Effective(day_count(s.day_count)),
        LoanMethod::Flat => billo_domain::LoanMethod::Flat,
    };
    if s.installment_minor.is_some_and(|m| m <= 0) {
        return Err(ApiError::validation(
            "rules.loan.installment_minor: must be positive",
        ));
    }
    LoanTerms::new(billo_domain::LoanSettings {
        principal: billo_domain::Money::new(s.principal_minor, currency),
        annual_rate_bp: s.annual_rate_bp,
        months: s.months,
        start: s.start,
        first_payment: s.first_payment,
        method,
        rounding: rounding("rules.loan.rounding", s.rounding)?,
        installment: s
            .installment_minor
            .map(|m| billo_domain::Money::new(m, currency)),
    })
    .map_err(|e| ApiError::validation(format!("rules.loan: {e}")))
}
