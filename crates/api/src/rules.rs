//! Account settings stored in `accounts.rules`, versioned by `v`.
//!
//! These are plain wire types. The server converts them into the `billo-domain` rule
//! types, whose constructors do the validation, and rejects anything they refuse.
//! Amounts here are minor units in the account's currency.

use serde::{Deserialize, Serialize};
use time::Date;

/// The current settings shape.
pub const RULES_VERSION: u32 = 1;

/// `{"v": 1}` for accounts without settings, `{"v": 1, "card": {...}}` for cards and
/// revolving credit, `{"v": 1, "loan": {...}}` for loans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRules {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<CardSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loan: Option<LoanSettings>,
}

impl Default for AccountRules {
    fn default() -> Self {
        Self {
            v: RULES_VERSION,
            card: None,
            loan: None,
        }
    }
}

/// Round to a multiple of `step` minor units (1 = satang, 100 = whole baht).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundingSettings {
    pub step: i64,
    pub mode: RoundingMode,
}

impl Default for RoundingSettings {
    fn default() -> Self {
        Self {
            step: 1,
            mode: RoundingMode::HalfUp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingMode {
    Up,
    Down,
    #[default]
    HalfUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayCount {
    #[default]
    Actual365,
    ActualActual,
    Actual360,
}

/// Credit card / revolving credit settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSettings {
    /// Day of month the statement is cut (1–31, clamped to month end).
    pub statement_day: u8,
    pub due: DueRule,
    pub due_shift: DueShift,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit_limit_minor: Option<i64>,
    pub min_payment: MinPaymentSettings,
    pub interest: InterestSettings,
}

/// `{"days_after_statement": 20}` or `{"day_of_month": 5}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueRule {
    DaysAfterStatement(u16),
    DayOfMonth(u8),
}

/// What happens when the due date isn't a business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueShift {
    None,
    Next,
    Previous,
}

/// "8% or ฿1,000, whichever is higher".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinPaymentSettings {
    /// 800 = 8%.
    pub percent_bp: u32,
    pub floor_minor: i64,
    #[serde(default)]
    pub rounding: RoundingSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestSettings {
    /// 1600 = 16% a year.
    pub annual_rate_bp: u32,
    #[serde(default)]
    pub day_count: DayCount,
    pub start: InterestStart,
    #[serde(default)]
    pub rounding: RoundingSettings,
    pub grace_if_paid_in_full: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterestStart {
    TransactionDate,
    PostingDate,
}

/// Loan settings, as on the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoanSettings {
    pub principal_minor: i64,
    /// 1500 = 15% a year (flat or effective, per `method`).
    pub annual_rate_bp: u32,
    pub months: u16,
    pub start: Date,
    pub first_payment: Date,
    pub method: LoanMethod,
    /// Used by the effective-rate method.
    #[serde(default)]
    pub day_count: DayCount,
    #[serde(default)]
    pub rounding: RoundingSettings,
    /// The bank's stated monthly amount, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installment_minor: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoanMethod {
    Effective,
    Flat,
}
