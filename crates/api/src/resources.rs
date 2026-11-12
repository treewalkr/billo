//! Resource fields: what a client sends in a [`Put`](crate::Put) and gets back in a
//! [`Record`](crate::Record).

use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use crate::rules::AccountRules;

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
