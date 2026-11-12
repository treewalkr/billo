//! Form state as plain strings, and turning it into API data. Kept free of UI code so
//! it can be unit-tested; every check that needs domain rules goes through
//! [`crate::convert`].

use billo_api::rules::{
    AccountRules, CardSettings, DayCount, DueRule, DueShift, InterestSettings, InterestStart,
    LoanMethod, LoanSettings, MinPaymentSettings, RULES_VERSION, RoundingMode, RoundingSettings,
};
use billo_api::{AccountData, AccountKind};
use billo_domain::Currency;
use serde::Serialize;
use serde::de::DeserializeOwned;
use time::{Date, Duration};

use crate::input::{field, optional, parse_amount, parse_date, parse_int, parse_percent_bp};
use crate::{convert, format};

/// The wire name of a settings enum (`"half_up"`), used as a `<select>` value.
pub fn key<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// Parses a `<select>` value back; unknown values are ignored by the caller.
pub fn from_key<T: DeserializeOwned>(k: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(k.to_owned())).ok()
}

pub const CURRENCIES: [&str; 4] = ["THB", "USD", "EUR", "JPY"];

/// How the due date is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueMode {
    DaysAfter,
    DayOfMonth,
}

/// Everything the account form edits. Strings are what the user typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDraft {
    pub kind: AccountKind,
    pub name: String,
    pub currency: String,
    pub opened_on: Option<Date>,
    pub closed_on: Option<Date>,

    // Card and revolving credit.
    pub statement_day: String,
    pub due_mode: DueMode,
    pub due_value: String,
    pub due_shift: DueShift,
    pub credit_limit: String,
    pub min_percent: String,
    pub min_floor: String,
    pub min_round_step: String,
    pub min_round_mode: RoundingMode,
    pub rate: String,
    pub day_count: DayCount,
    pub interest_start: InterestStart,
    pub grace: bool,
    pub interest_round_step: String,
    pub interest_round_mode: RoundingMode,

    // Loans.
    pub principal: String,
    pub loan_rate: String,
    pub months: String,
    pub start: String,
    pub first_payment: String,
    pub method: LoanMethod,
    pub loan_day_count: DayCount,
    pub loan_rounding: RoundingSettings,
    pub installment: String,
}

pub fn is_card_like(kind: AccountKind) -> bool {
    matches!(kind, AccountKind::Card | AccountKind::Revolving)
}

impl AccountDraft {
    /// A new account of `kind`, pre-filled with example settings for the user to edit.
    /// These are examples, not any bank's terms.
    pub fn new(kind: AccountKind, today: Date) -> Self {
        let next_month = today.checked_add(Duration::days(30)).unwrap_or(today);
        Self {
            kind,
            name: String::new(),
            currency: "THB".into(),
            opened_on: None,
            closed_on: None,
            statement_day: "25".into(),
            due_mode: DueMode::DaysAfter,
            due_value: "20".into(),
            due_shift: DueShift::Next,
            credit_limit: String::new(),
            min_percent: "8".into(),
            min_floor: "1000.00".into(),
            min_round_step: "0.01".into(),
            min_round_mode: RoundingMode::HalfUp,
            rate: if kind == AccountKind::Revolving {
                "25"
            } else {
                "16"
            }
            .into(),
            day_count: DayCount::Actual365,
            interest_start: InterestStart::TransactionDate,
            grace: kind == AccountKind::Card,
            interest_round_step: "0.01".into(),
            interest_round_mode: RoundingMode::HalfUp,
            principal: String::new(),
            loan_rate: "25".into(),
            months: "12".into(),
            start: today.to_string(),
            first_payment: next_month.to_string(),
            method: LoanMethod::Effective,
            loan_day_count: DayCount::Actual365,
            loan_rounding: RoundingSettings::default(),
            installment: String::new(),
        }
    }

    /// The form for editing a stored account; settings it doesn't have keep the
    /// examples from [`AccountDraft::new`].
    pub fn from_data(d: &AccountData, today: Date) -> Self {
        let mut f = Self::new(d.kind, today);
        f.name = d.name.clone();
        f.currency = d.currency.clone();
        f.opened_on = d.opened_on;
        f.closed_on = d.closed_on;
        let cur = Currency::from_code(&d.currency).unwrap_or(Currency::Thb);
        let amount = |m: i64| format::plain_amount(m, cur);
        if let Some(c) = &d.rules.card {
            f.statement_day = c.statement_day.to_string();
            (f.due_mode, f.due_value) = match c.due {
                DueRule::DaysAfterStatement(n) => (DueMode::DaysAfter, n.to_string()),
                DueRule::DayOfMonth(n) => (DueMode::DayOfMonth, n.to_string()),
            };
            f.due_shift = c.due_shift;
            f.credit_limit = c.credit_limit_minor.map(amount).unwrap_or_default();
            f.min_percent = format::percent(c.min_payment.percent_bp);
            f.min_floor = amount(c.min_payment.floor_minor);
            f.min_round_step = amount(c.min_payment.rounding.step);
            f.min_round_mode = c.min_payment.rounding.mode;
            f.rate = format::percent(c.interest.annual_rate_bp);
            f.day_count = c.interest.day_count;
            f.interest_start = c.interest.start;
            f.grace = c.interest.grace_if_paid_in_full;
            f.interest_round_step = amount(c.interest.rounding.step);
            f.interest_round_mode = c.interest.rounding.mode;
        }
        if let Some(l) = &d.rules.loan {
            f.principal = amount(l.principal_minor);
            f.loan_rate = format::percent(l.annual_rate_bp);
            f.months = l.months.to_string();
            f.start = l.start.to_string();
            f.first_payment = l.first_payment.to_string();
            f.method = l.method;
            f.loan_day_count = l.day_count;
            f.loan_rounding = l.rounding;
            f.installment = l.installment_minor.map(amount).unwrap_or_default();
        }
        f
    }

    /// Validates the form and builds the account. Errors name the field.
    pub fn to_data(&self) -> Result<AccountData, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Name: required".into());
        }
        let cur = convert::currency(&self.currency).map_err(|e| e.to_string())?;
        let mut rules = AccountRules {
            v: RULES_VERSION,
            card: None,
            loan: None,
        };
        if is_card_like(self.kind) {
            let card = self.card_settings(cur)?;
            convert::card_rules(&card, cur).map_err(|e| e.to_string())?;
            rules.card = Some(card);
        }
        if self.kind == AccountKind::Loan {
            let loan = self.loan_settings(cur)?;
            convert::loan_terms(&loan, cur).map_err(|e| e.to_string())?;
            rules.loan = Some(loan);
        }
        Ok(AccountData {
            kind: self.kind,
            name: name.to_owned(),
            currency: cur.code().to_owned(),
            rules,
            opened_on: self.opened_on,
            closed_on: self.closed_on,
        })
    }

    fn card_settings(&self, cur: Currency) -> Result<CardSettings, String> {
        let day = |label: &str, t: &str| -> Result<u8, String> {
            let d = field(label, parse_int(t, 1, 31))?;
            u8::try_from(d).map_err(|_| format!("{label}: must be 1 to 31"))
        };
        let statement_day = day("Statement day", &self.statement_day)?;
        let due = match self.due_mode {
            DueMode::DaysAfter => {
                let n = field("Days after statement", parse_int(&self.due_value, 1, 90))?;
                DueRule::DaysAfterStatement(
                    u16::try_from(n).map_err(|_| "Days after statement: too large".to_owned())?,
                )
            }
            DueMode::DayOfMonth => DueRule::DayOfMonth(day("Due day", &self.due_value)?),
        };
        let amount = |label: &str, t: &str| field(label, parse_amount(t, cur, false));
        Ok(CardSettings {
            statement_day,
            due,
            due_shift: self.due_shift,
            credit_limit_minor: field(
                "Credit limit",
                optional(&self.credit_limit, |t| parse_amount(t, cur, false)),
            )?,
            min_payment: MinPaymentSettings {
                percent_bp: field("Minimum %", parse_percent_bp(&self.min_percent))?,
                floor_minor: amount("Minimum amount", &self.min_floor)?,
                rounding: RoundingSettings {
                    step: amount("Minimum rounding", &self.min_round_step)?,
                    mode: self.min_round_mode,
                },
            },
            interest: InterestSettings {
                annual_rate_bp: field("Interest rate", parse_percent_bp(&self.rate))?,
                day_count: self.day_count,
                start: self.interest_start,
                rounding: RoundingSettings {
                    step: amount("Interest rounding", &self.interest_round_step)?,
                    mode: self.interest_round_mode,
                },
                grace_if_paid_in_full: self.grace,
            },
        })
    }

    fn loan_settings(&self, cur: Currency) -> Result<LoanSettings, String> {
        let months = field("Months", parse_int(&self.months, 1, 600))?;
        Ok(LoanSettings {
            principal_minor: field("Amount borrowed", parse_amount(&self.principal, cur, false))?,
            annual_rate_bp: field("Interest rate", parse_percent_bp(&self.loan_rate))?,
            months: u16::try_from(months).map_err(|_| "Months: too large".to_owned())?,
            start: field("Start date", parse_date(&self.start))?,
            first_payment: field("First payment", parse_date(&self.first_payment))?,
            method: self.method,
            day_count: self.loan_day_count,
            rounding: self.loan_rounding,
            installment_minor: field(
                "Monthly installment",
                optional(&self.installment, |t| parse_amount(t, cur, false)),
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const TODAY: Date = date!(2026 - 11 - 12);

    #[test]
    fn select_keys_round_trip() {
        assert_eq!(key(&RoundingMode::HalfUp), "half_up");
        assert_eq!(
            from_key::<RoundingMode>("half_up"),
            Some(RoundingMode::HalfUp)
        );
        assert_eq!(key(&AccountKind::Revolving), "revolving");
        assert_eq!(from_key::<DueShift>("nope"), None);
    }

    #[test]
    fn card_draft_builds_settings() {
        let mut f = AccountDraft::new(AccountKind::Card, TODAY);
        f.name = "  My card ".into();
        f.credit_limit = "50,000".into();
        f.min_round_step = "1".into();
        f.min_round_mode = RoundingMode::Up;
        let d = f.to_data().unwrap();
        assert_eq!(d.name, "My card");
        assert_eq!(d.currency, "THB");
        let c = d.rules.card.unwrap();
        assert_eq!(c.statement_day, 25);
        assert_eq!(c.due, DueRule::DaysAfterStatement(20));
        assert_eq!(c.credit_limit_minor, Some(5_000_000));
        assert_eq!(c.min_payment.percent_bp, 800);
        assert_eq!(c.min_payment.floor_minor, 100_000);
        assert_eq!(
            c.min_payment.rounding,
            RoundingSettings {
                step: 100,
                mode: RoundingMode::Up
            }
        );
        assert_eq!(c.interest.annual_rate_bp, 1_600);
        assert!(c.interest.grace_if_paid_in_full);
        assert!(d.rules.loan.is_none());
    }

    #[test]
    fn card_draft_round_trips_through_data() {
        let mut f = AccountDraft::new(AccountKind::Card, TODAY);
        f.name = "Card".into();
        f.due_mode = DueMode::DayOfMonth;
        f.due_value = "5".into();
        f.rate = "16.5".into();
        let d = f.to_data().unwrap();
        let back = AccountDraft::from_data(&d, TODAY);
        assert_eq!(back.due_mode, DueMode::DayOfMonth);
        assert_eq!(back.due_value, "5");
        assert_eq!(back.rate, "16.5");
        assert_eq!(back.min_floor, "1000.00");
        assert_eq!(back.to_data().unwrap(), d);
    }

    #[test]
    fn card_draft_errors_name_the_field() {
        let mut f = AccountDraft::new(AccountKind::Card, TODAY);
        f.name = "Card".into();
        f.statement_day = "32".into();
        assert_eq!(f.to_data().unwrap_err(), "Statement day: must be 1 to 31");
        f.statement_day = "25".into();
        f.min_floor = "abc".into();
        assert_eq!(f.to_data().unwrap_err(), "Minimum amount: not a number");
        f.min_floor = "1000".into();
        f.name = " ".into();
        assert_eq!(f.to_data().unwrap_err(), "Name: required");
    }

    #[test]
    fn loan_draft_builds_settings() {
        let mut f = AccountDraft::new(AccountKind::Loan, TODAY);
        f.name = "Loan".into();
        f.principal = "100,000".into();
        f.loan_rate = "15".into();
        f.first_payment = "2026-12-12".into();
        let d = f.to_data().unwrap();
        let l = d.rules.loan.unwrap();
        assert_eq!(l.principal_minor, 10_000_000);
        assert_eq!(l.annual_rate_bp, 1_500);
        assert_eq!(l.months, 12);
        assert_eq!(l.start, TODAY);
        assert_eq!(l.first_payment, date!(2026 - 12 - 12));
        assert_eq!(l.installment_minor, None);
        assert!(d.rules.card.is_none());

        f.principal = String::new();
        assert_eq!(f.to_data().unwrap_err(), "Amount borrowed: required");
    }

    #[test]
    fn cash_draft_has_no_settings() {
        let mut f = AccountDraft::new(AccountKind::Cash, TODAY);
        f.name = "Wallet".into();
        f.currency = "JPY".into();
        let d = f.to_data().unwrap();
        assert_eq!(d.currency, "JPY");
        assert_eq!(d.rules, AccountRules::default());
    }
}
