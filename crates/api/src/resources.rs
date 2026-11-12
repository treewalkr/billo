//! Resource fields: what a client sends in a [`Put`](crate::Put) and gets back in a
//! [`Record`](crate::Record).

use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use crate::Money;
use crate::rules::{AccountRules, RoundingSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Cash,
    Bank,
    Card,
    Revolving,
    Loan,
}

/// `/api/v1/accounts`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountData {
    pub kind: AccountKind,
    pub name: String,
    /// ISO 4217 code, e.g. `"THB"`.
    pub currency: String,
    #[serde(default)]
    pub rules: AccountRules,
    #[serde(default)]
    pub opened_on: Option<Date>,
    #[serde(default)]
    pub closed_on: Option<Date>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryKind {
    Income,
    Expense,
}

/// `/api/v1/categories`. At least one of the names is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryData {
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub name_th: Option<String>,
    #[serde(default)]
    pub name_en: Option<String>,
    pub kind: CategoryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Expense,
    Income,
    Transfer,
    Payment,
    Fee,
    InterestAdjust,
}

/// `/api/v1/transactions`. `amount` is in the account's currency and positive; `kind`
/// gives the direction (only `interest_adjust` may be negative).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionData {
    pub account_id: Uuid,
    #[serde(default)]
    pub category_id: Option<Uuid>,
    pub occurred_on: Date,
    #[serde(default)]
    pub posted_on: Option<Date>,
    pub amount: Money,
    /// The original amount when spent in another currency.
    #[serde(default)]
    pub orig_amount: Option<Money>,
    pub kind: TransactionKind,
    /// The other leg of a transfer.
    #[serde(default)]
    pub transfer_peer_id: Option<Uuid>,
    /// Set on the portions an installment plan posts.
    #[serde(default)]
    pub installment_plan_id: Option<Uuid>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Query parameters of `GET /api/v1/transactions`. Dates are inclusive and filter on
/// `occurred_on`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Date>,
}

/// Which portion absorbs the rounding remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Remainder {
    First,
    #[default]
    Last,
}

/// `/api/v1/installment-plans`: a purchase split into monthly portions on a card
/// (ผ่อนชำระ), at a flat monthly rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallmentPlanData {
    pub card_account_id: Uuid,
    pub principal: Money,
    pub months: u16,
    /// Flat rate per month in basis points: 69 = 0.69%, 0 for 0% plans.
    pub monthly_rate_bp: u32,
    pub first_post_on: Date,
    #[serde(default)]
    pub closed_on: Option<Date>,
    #[serde(default)]
    pub rounding: RoundingSettings,
    #[serde(default)]
    pub remainder: Remainder,
}

/// `/api/v1/budgets`: a monthly spending limit for a category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetData {
    pub category_id: Uuid,
    /// First day of the month, e.g. `"2026-11-01"`.
    pub period_month: Date,
    pub limit: Money,
}

/// Query parameters of `GET /api/v1/budgets`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_month: Option<Date>,
}
