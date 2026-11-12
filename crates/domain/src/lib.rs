//! Billo domain: pure, deterministic financial calculations.
//!
//! No I/O, no clock, no randomness. Every input (including "today") is passed in.

pub mod budget;
pub mod money;
pub mod planner;

pub use budget::{
    Budget, BudgetError, CategoryBudget, MonthlyBudget, SpendingItem, SpendingKind, monthly_budget,
};
pub use money::{Currency, Money, MoneyError};

pub mod calendar;

pub use calendar::{BusinessCalendar, EveryDay, WeekendCalendar};

pub mod holidays;

pub mod card;
pub mod daycount;
pub mod loan;

pub mod rounding;

pub use daycount::DayCount;
pub use loan::{
    ExtraPayment, LoanError, LoanMethod, LoanPayment, LoanSettings, LoanTerms, PaymentKind,
};
pub use rounding::{Rounding, RoundingMode};
