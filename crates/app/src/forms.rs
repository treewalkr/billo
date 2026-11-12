//! Form state as plain strings, and turning it into API data. Kept free of UI code so
//! it can be unit-tested; every check that needs domain rules goes through
//! [`crate::convert`].

use billo_api::rules::{
    AccountRules, CardSettings, DayCount, DueRule, DueShift, InterestSettings, InterestStart,
    LoanMethod, LoanSettings, MinPaymentSettings, RULES_VERSION, RoundingMode, RoundingSettings,
};
use billo_api::{
    AccountData, AccountKind, CategoryData, CategoryKind, InstallmentPlanData, TransactionData,
    TransactionKind,
};
use billo_domain::Currency;
use serde::Serialize;
use serde::de::DeserializeOwned;
use time::{Date, Duration};
use uuid::Uuid;

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

/// Which transaction kinds make sense for an account, in menu order.
pub fn transaction_kinds(account: AccountKind) -> &'static [TransactionKind] {
    match account {
        AccountKind::Card | AccountKind::Revolving => &[
            TransactionKind::Expense,
            TransactionKind::Payment,
            TransactionKind::Fee,
            TransactionKind::Income,
            TransactionKind::InterestAdjust,
        ],
        AccountKind::Loan => &[TransactionKind::Payment, TransactionKind::Fee],
        AccountKind::Cash | AccountKind::Bank => &[
            TransactionKind::Expense,
            TransactionKind::Income,
            TransactionKind::Fee,
        ],
    }
}

/// Category kind matching a transaction kind (income vs. spending). A refund on a
/// card goes back to a spending category, so it reduces that category's spending.
pub fn category_kind(account: AccountKind, kind: TransactionKind) -> CategoryKind {
    match kind {
        TransactionKind::Income if !is_card_like(account) => CategoryKind::Income,
        _ => CategoryKind::Expense,
    }
}

/// `<select>` value for "create a new category".
pub const NEW_CATEGORY: &str = "new";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionDraft {
    pub kind: TransactionKind,
    pub occurred_on: String,
    pub posted_on: String,
    pub amount: String,
    /// A category id, `""` for none, or [`NEW_CATEGORY`].
    pub category: String,
    pub new_category: String,
    pub note: String,
    /// Fields the form doesn't edit, kept as they were.
    pub keep: Option<TransactionData>,
}

impl TransactionDraft {
    pub fn new(account: AccountKind, today: Date) -> Self {
        Self {
            kind: transaction_kinds(account)[0],
            occurred_on: today.to_string(),
            posted_on: String::new(),
            amount: String::new(),
            category: String::new(),
            new_category: String::new(),
            note: String::new(),
            keep: None,
        }
    }

    pub fn from_data(d: &TransactionData) -> Self {
        let cur = Currency::from_code(&d.amount.currency).unwrap_or(Currency::Thb);
        Self {
            kind: d.kind,
            occurred_on: d.occurred_on.to_string(),
            posted_on: d.posted_on.map(|p| p.to_string()).unwrap_or_default(),
            amount: format::plain_amount(d.amount.minor, cur),
            category: d.category_id.map(|c| c.to_string()).unwrap_or_default(),
            new_category: String::new(),
            note: d.note.clone().unwrap_or_default(),
            keep: Some(d.clone()),
        }
    }

    /// The new category's name, when the user chose to create one.
    pub fn new_category_name(&self) -> Result<Option<String>, String> {
        if self.category != NEW_CATEGORY {
            return Ok(None);
        }
        let name = self.new_category.trim();
        if name.is_empty() {
            return Err("New category: required".into());
        }
        Ok(Some(name.to_owned()))
    }

    /// Builds the transaction. `new_category_id` is the id of a category created for
    /// this transaction, if any. Posting dates only apply to cards; blank means the
    /// same day as the purchase.
    pub fn to_data(
        &self,
        account_id: Uuid,
        account: AccountKind,
        currency: &str,
        new_category_id: Option<Uuid>,
    ) -> Result<TransactionData, String> {
        let cur = convert::currency(currency).map_err(|e| e.to_string())?;
        let occurred_on = field("Date", parse_date(&self.occurred_on))?;
        let posted_on = if is_card_like(account) {
            field("Posting date", optional(&self.posted_on, parse_date))?
        } else {
            None
        };
        if posted_on.is_some_and(|p| p < occurred_on) {
            return Err("Posting date: can't be before the date".into());
        }
        let negative_ok = self.kind == TransactionKind::InterestAdjust;
        let minor = field("Amount", parse_amount(&self.amount, cur, negative_ok))?;
        if minor == 0 || (!negative_ok && minor < 0) {
            return Err("Amount: must be more than zero".into());
        }
        let category_id = match (self.category.as_str(), new_category_id) {
            (NEW_CATEGORY, id) => id,
            ("", _) => None,
            (id, _) => Some(Uuid::parse_str(id).map_err(|_| "Category: unknown".to_owned())?),
        };
        let note = self.note.trim();
        let keep = self.keep.as_ref();
        Ok(TransactionData {
            account_id,
            category_id,
            occurred_on,
            posted_on,
            amount: billo_api::Money::new(minor, cur.code()),
            orig_amount: keep.and_then(|k| k.orig_amount.clone()),
            kind: self.kind,
            transfer_peer_id: keep.and_then(|k| k.transfer_peer_id),
            installment_plan_id: keep.and_then(|k| k.installment_plan_id),
            note: (!note.is_empty()).then(|| note.to_owned()),
        })
    }
}

/// An installment plan on a card: principal, months, flat monthly rate, first posting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDraft {
    pub principal: String,
    pub months: String,
    pub monthly_rate: String,
    pub first_post_on: String,
    pub keep: Option<InstallmentPlanData>,
}

impl PlanDraft {
    pub fn new(today: Date) -> Self {
        Self {
            principal: String::new(),
            months: "10".into(),
            monthly_rate: "0".into(),
            first_post_on: today.to_string(),
            keep: None,
        }
    }

    pub fn from_data(d: &InstallmentPlanData) -> Self {
        let cur = Currency::from_code(&d.principal.currency).unwrap_or(Currency::Thb);
        Self {
            principal: format::plain_amount(d.principal.minor, cur),
            months: d.months.to_string(),
            monthly_rate: format::percent(d.monthly_rate_bp),
            first_post_on: d.first_post_on.to_string(),
            keep: Some(d.clone()),
        }
    }

    pub fn to_data(&self, card_id: Uuid, currency: &str) -> Result<InstallmentPlanData, String> {
        let cur = convert::currency(currency).map_err(|e| e.to_string())?;
        let months = field("Months", parse_int(&self.months, 1, 120))?;
        let keep = self.keep.as_ref();
        let data = InstallmentPlanData {
            card_account_id: card_id,
            principal: billo_api::Money::new(
                field("Amount", parse_amount(&self.principal, cur, false))?,
                cur.code(),
            ),
            months: u16::try_from(months).map_err(|_| "Months: too large".to_owned())?,
            monthly_rate_bp: field("Monthly rate", parse_percent_bp(&self.monthly_rate))?,
            first_post_on: field("First posting", parse_date(&self.first_post_on))?,
            closed_on: keep.and_then(|k| k.closed_on),
            rounding: keep.map(|k| k.rounding).unwrap_or_default(),
            remainder: keep.map(|k| k.remainder).unwrap_or_default(),
        };
        convert::installment_plan(&data).map_err(|e| e.to_string())?;
        Ok(data)
    }
}

/// A category: Thai and English names (at least one), kind, and optional parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryDraft {
    pub name_th: String,
    pub name_en: String,
    pub kind: CategoryKind,
    /// A parent category id, or `""` for none.
    pub parent: String,
}

impl CategoryDraft {
    pub fn new(kind: CategoryKind) -> Self {
        Self {
            name_th: String::new(),
            name_en: String::new(),
            kind,
            parent: String::new(),
        }
    }

    pub fn from_data(d: &CategoryData) -> Self {
        Self {
            name_th: d.name_th.clone().unwrap_or_default(),
            name_en: d.name_en.clone().unwrap_or_default(),
            kind: d.kind,
            parent: d.parent_id.map(|p| p.to_string()).unwrap_or_default(),
        }
    }

    pub fn to_data(&self) -> Result<CategoryData, String> {
        let text = |t: &str| {
            let t = t.trim();
            (!t.is_empty()).then(|| t.to_owned())
        };
        let (name_th, name_en) = (text(&self.name_th), text(&self.name_en));
        if name_th.is_none() && name_en.is_none() {
            return Err(crate::strings::CATEGORY_NAME_REQUIRED.to_owned());
        }
        let parent_id = match self.parent.as_str() {
            "" => None,
            id => Some(Uuid::parse_str(id).map_err(|_| "Parent: unknown".to_owned())?),
        };
        Ok(CategoryData {
            parent_id,
            name_th,
            name_en,
            kind: self.kind,
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

    fn txn_draft(amount: &str) -> TransactionDraft {
        TransactionDraft {
            amount: amount.into(),
            ..TransactionDraft::new(AccountKind::Card, TODAY)
        }
    }

    #[test]
    fn transaction_draft_builds_data() {
        let card = Uuid::nil();
        let mut f = txn_draft("1,234.50");
        f.posted_on = "2026-11-13".into();
        f.note = "  shoes ".into();
        let d = f.to_data(card, AccountKind::Card, "THB", None).unwrap();
        assert_eq!(d.kind, TransactionKind::Expense);
        assert_eq!(d.amount, billo_api::Money::new(123_450, "THB"));
        assert_eq!(d.occurred_on, TODAY);
        assert_eq!(d.posted_on, Some(date!(2026 - 11 - 13)));
        assert_eq!(d.note.as_deref(), Some("shoes"));
        assert_eq!(d.category_id, None);
        assert_eq!(
            TransactionDraft::from_data(&d).to_data(card, AccountKind::Card, "THB", None),
            Ok(d)
        );
    }

    #[test]
    fn transaction_draft_checks_amount_and_dates() {
        let card = Uuid::nil();
        assert_eq!(
            txn_draft("0").to_data(card, AccountKind::Card, "THB", None),
            Err("Amount: must be more than zero".into())
        );
        assert_eq!(
            txn_draft("-5").to_data(card, AccountKind::Card, "THB", None),
            Err("Amount: can't be negative".into())
        );
        let mut f = txn_draft("-5");
        f.kind = TransactionKind::InterestAdjust;
        assert_eq!(
            f.to_data(card, AccountKind::Card, "THB", None)
                .unwrap()
                .amount
                .minor,
            -500
        );
        let mut f = txn_draft("5");
        f.posted_on = "2026-11-01".into();
        assert!(f.to_data(card, AccountKind::Card, "THB", None).is_err());
        // Posting dates are ignored outside cards.
        assert_eq!(
            f.to_data(card, AccountKind::Bank, "THB", None)
                .unwrap()
                .posted_on,
            None
        );
    }

    #[test]
    fn transaction_draft_categories() {
        let card = Uuid::nil();
        let cat = Uuid::from_u128(7);
        let mut f = txn_draft("5");
        f.category = cat.to_string();
        assert_eq!(
            f.to_data(card, AccountKind::Card, "THB", None)
                .unwrap()
                .category_id,
            Some(cat)
        );
        f.category = NEW_CATEGORY.into();
        assert_eq!(f.new_category_name(), Err("New category: required".into()));
        f.new_category = " Food ".into();
        assert_eq!(f.new_category_name(), Ok(Some("Food".into())));
        assert_eq!(
            f.to_data(card, AccountKind::Card, "THB", Some(cat))
                .unwrap()
                .category_id,
            Some(cat)
        );
    }

    #[test]
    fn plan_draft_builds_data() {
        let card = Uuid::nil();
        let f = PlanDraft {
            principal: "12,000".into(),
            months: "6".into(),
            monthly_rate: "0.69".into(),
            first_post_on: "2026-10-15".into(),
            keep: None,
        };
        let d = f.to_data(card, "THB").unwrap();
        assert_eq!(d.principal, billo_api::Money::new(1_200_000, "THB"));
        assert_eq!(d.months, 6);
        assert_eq!(d.monthly_rate_bp, 69);
        assert_eq!(d.first_post_on, date!(2026 - 10 - 15));
        assert_eq!(PlanDraft::from_data(&d).to_data(card, "THB"), Ok(d));

        let bad = PlanDraft {
            months: "0".into(),
            ..f
        };
        assert_eq!(
            bad.to_data(card, "THB"),
            Err("Months: must be 1 to 120".into())
        );
    }

    #[test]
    fn category_draft_needs_a_name() {
        let mut f = CategoryDraft::new(CategoryKind::Expense);
        f.name_en = "  ".into();
        assert!(f.to_data().is_err());
        f.name_th = " อาหาร ".into();
        let d = f.to_data().unwrap();
        assert_eq!(d.name_th.as_deref(), Some("อาหาร"));
        assert_eq!(d.name_en, None);
        assert_eq!(d.parent_id, None);
        assert_eq!(CategoryDraft::from_data(&d), {
            let mut g = CategoryDraft::new(CategoryKind::Expense);
            g.name_th = "อาหาร".into();
            g
        });
        f.parent = Uuid::nil().to_string();
        assert_eq!(f.to_data().unwrap().parent_id, Some(Uuid::nil()));
        f.parent = "bad".into();
        assert!(f.to_data().is_err());
    }

    #[test]
    fn card_refunds_use_spending_categories() {
        use CategoryKind::{Expense, Income};
        let k = |a, t| category_kind(a, t);
        assert_eq!(k(AccountKind::Bank, TransactionKind::Income), Income);
        assert_eq!(k(AccountKind::Card, TransactionKind::Income), Expense);
        assert_eq!(k(AccountKind::Cash, TransactionKind::Expense), Expense);
    }
}
