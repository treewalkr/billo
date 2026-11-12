//! Monthly category budgets vs. actual spending.
//!
//! Categories are whatever id type the caller uses (`C: Ord + Clone`); the domain does
//! not know or hardcode any category list. Only expenses count as spending, refunds
//! reduce spending in their category, and income is ignored.

use std::collections::BTreeMap;
use std::fmt;

use time::{Date, Month};

use crate::money::{Currency, Money, MoneyError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetError {
    /// A budget limit was below zero.
    NegativeLimit,
    /// The same category was budgeted twice for the month.
    DuplicateCategory,
    /// A spending item had a zero or negative amount (direction comes from its kind).
    NonPositiveAmount,
    Money(MoneyError),
}

impl From<MoneyError> for BudgetError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeLimit => f.write_str("budget limit cannot be negative"),
            Self::DuplicateCategory => f.write_str("category is budgeted more than once"),
            Self::NonPositiveAmount => f.write_str("spending amount must be positive"),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BudgetError {}

/// A spending limit for one category in a month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget<C> {
    pub category: C,
    pub limit: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendingKind {
    Expense,
    Income,
    Refund,
}

/// One money movement. `amount` is always positive; `kind` gives the direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendingItem<C> {
    pub category: C,
    pub date: Date,
    pub amount: Money,
    pub kind: SpendingKind,
}

/// Budget vs. actual for one budgeted category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryBudget<C> {
    pub category: C,
    pub limit: Money,
    /// Expenses minus refunds in the month. Negative if refunds exceed expenses.
    pub spent: Money,
    /// `limit - spent`; negative when over budget.
    pub remaining: Money,
    /// Share of the limit spent, in basis points (10_000 = 100%). `None` for a zero limit.
    pub utilisation_bp: Option<u32>,
    pub over_budget: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyBudget<C> {
    pub year: i32,
    pub month: Month,
    /// One entry per budget, sorted by category.
    pub categories: Vec<CategoryBudget<C>>,
    /// Sum of limits of budgeted categories.
    pub total_limit: Money,
    /// Net spending in budgeted categories.
    pub total_spent: Money,
    /// `total_limit - total_spent`.
    pub total_remaining: Money,
    /// Net spending in categories that have no budget this month.
    pub unbudgeted_spent: Money,
}

/// Compares `budgets` with `items` dated in `year`/`month`.
///
/// Every amount must be in `currency`. Items outside the month are ignored but still
/// validated, so bad data is reported whichever month is viewed.
pub fn monthly_budget<C: Ord + Clone>(
    year: i32,
    month: Month,
    currency: Currency,
    budgets: &[Budget<C>],
    items: &[SpendingItem<C>],
) -> Result<MonthlyBudget<C>, BudgetError> {
    let zero = Money::zero(currency);

    let mut limits: BTreeMap<&C, Money> = BTreeMap::new();
    for b in budgets {
        zero.checked_add(b.limit)?;
        if b.limit.is_negative() {
            return Err(BudgetError::NegativeLimit);
        }
        if limits.insert(&b.category, b.limit).is_some() {
            return Err(BudgetError::DuplicateCategory);
        }
    }

    let mut spent: BTreeMap<&C, Money> = BTreeMap::new();
    let mut unbudgeted_spent = zero;
    for item in items {
        zero.checked_add(item.amount)?;
        if item.amount.minor() <= 0 {
            return Err(BudgetError::NonPositiveAmount);
        }
        if item.date.year() != year || item.date.month() != month {
            continue;
        }
        let delta = match item.kind {
            SpendingKind::Expense => item.amount,
            SpendingKind::Refund => item.amount.checked_neg()?,
            SpendingKind::Income => continue,
        };
        if limits.contains_key(&item.category) {
            let s = spent.entry(&item.category).or_insert(zero);
            *s = s.checked_add(delta)?;
        } else {
            unbudgeted_spent = unbudgeted_spent.checked_add(delta)?;
        }
    }

    let mut categories = Vec::with_capacity(limits.len());
    let mut total_limit = zero;
    let mut total_spent = zero;
    for (category, limit) in limits {
        let spent = spent.get(category).copied().unwrap_or(zero);
        let remaining = limit.checked_sub(spent)?;
        total_limit = total_limit.checked_add(limit)?;
        total_spent = total_spent.checked_add(spent)?;
        categories.push(CategoryBudget {
            category: category.clone(),
            limit,
            spent,
            remaining,
            utilisation_bp: utilisation_bp(limit, spent),
            over_budget: remaining.is_negative(),
        });
    }

    Ok(MonthlyBudget {
        year,
        month,
        categories,
        total_limit,
        total_spent,
        total_remaining: total_limit.checked_sub(total_spent)?,
        unbudgeted_spent,
    })
}

fn utilisation_bp(limit: Money, spent: Money) -> Option<u32> {
    if limit.is_zero() {
        return None;
    }
    let bp = i128::from(spent.minor().max(0)) * 10_000 / i128::from(limit.minor());
    Some(u32::try_from(bp).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    fn budget(category: &'static str, baht: i64) -> Budget<&'static str> {
        Budget {
            category,
            limit: thb(baht),
        }
    }

    fn item(
        category: &'static str,
        date: Date,
        baht: i64,
        kind: SpendingKind,
    ) -> SpendingItem<&'static str> {
        SpendingItem {
            category,
            date,
            amount: thb(baht),
            kind,
        }
    }

    fn expense(category: &'static str, date: Date, baht: i64) -> SpendingItem<&'static str> {
        item(category, date, baht, SpendingKind::Expense)
    }

    fn nov(
        budgets: &[Budget<&'static str>],
        items: &[SpendingItem<&'static str>],
    ) -> Result<MonthlyBudget<&'static str>, BudgetError> {
        monthly_budget(2026, Month::November, Currency::Thb, budgets, items)
    }

    #[test]
    fn spending_within_budget() {
        let r = nov(
            &[budget("food", 6_000)],
            &[
                expense("food", date!(2026 - 11 - 3), 1_200),
                expense("food", date!(2026 - 11 - 10), 300),
            ],
        )
        .unwrap();
        // spent 1,200 + 300 = 1,500; remaining 6,000 - 1,500 = 4,500; 1,500/6,000 = 25%
        assert_eq!(
            r.categories,
            vec![CategoryBudget {
                category: "food",
                limit: thb(6_000),
                spent: thb(1_500),
                remaining: thb(4_500),
                utilisation_bp: Some(2_500),
                over_budget: false,
            }]
        );
        assert_eq!(r.total_limit, thb(6_000));
        assert_eq!(r.total_spent, thb(1_500));
        assert_eq!(r.total_remaining, thb(4_500));
        assert_eq!(r.unbudgeted_spent, thb(0));
    }

    #[test]
    fn month_boundaries_are_inclusive_and_exclusive() {
        let r = nov(
            &[budget("food", 1_000)],
            &[
                expense("food", date!(2026 - 10 - 31), 100), // previous month
                expense("food", date!(2026 - 11 - 1), 10),
                expense("food", date!(2026 - 11 - 30), 20),
                expense("food", date!(2026 - 12 - 1), 400), // next month
                expense("food", date!(2025 - 11 - 15), 800), // same month, last year
            ],
        )
        .unwrap();
        // only 10 + 20 = 30
        assert_eq!(r.categories[0].spent, thb(30));
        assert_eq!(r.categories[0].remaining, thb(970));
    }

    #[test]
    fn refunds_reduce_spending_and_income_is_ignored() {
        let r = nov(
            &[budget("shopping", 5_000)],
            &[
                expense("shopping", date!(2026 - 11 - 2), 3_000),
                item(
                    "shopping",
                    date!(2026 - 11 - 9),
                    1_000,
                    SpendingKind::Refund,
                ),
                item(
                    "shopping",
                    date!(2026 - 11 - 25),
                    50_000,
                    SpendingKind::Income,
                ),
            ],
        )
        .unwrap();
        // 3,000 - 1,000 = 2,000 → 40%
        let c = &r.categories[0];
        assert_eq!(c.spent, thb(2_000));
        assert_eq!(c.remaining, thb(3_000));
        assert_eq!(c.utilisation_bp, Some(4_000));
    }

    #[test]
    fn refund_larger_than_spending_goes_negative() {
        // Refund this month for a purchase made last month.
        let r = nov(
            &[budget("shopping", 1_000)],
            &[item(
                "shopping",
                date!(2026 - 11 - 5),
                400,
                SpendingKind::Refund,
            )],
        )
        .unwrap();
        let c = &r.categories[0];
        assert_eq!(c.spent, thb(-400));
        assert_eq!(c.remaining, thb(1_400));
        assert_eq!(c.utilisation_bp, Some(0));
        assert!(!c.over_budget);
    }

    #[test]
    fn over_budget() {
        let r = nov(
            &[budget("travel", 2_000)],
            &[
                expense("travel", date!(2026 - 11 - 1), 1_500),
                expense("travel", date!(2026 - 11 - 20), 1_000),
            ],
        )
        .unwrap();
        // 2,500 spent of 2,000 → remaining -500, 125%
        let c = &r.categories[0];
        assert_eq!(c.remaining, thb(-500));
        assert_eq!(c.utilisation_bp, Some(12_500));
        assert!(c.over_budget);
        assert_eq!(r.total_remaining, thb(-500));
    }

    #[test]
    fn exactly_at_limit_is_not_over() {
        let r = nov(
            &[budget("bills", 800)],
            &[expense("bills", date!(2026 - 11 - 15), 800)],
        )
        .unwrap();
        let c = &r.categories[0];
        assert_eq!(c.remaining, thb(0));
        assert_eq!(c.utilisation_bp, Some(10_000));
        assert!(!c.over_budget);
    }

    #[test]
    fn unbudgeted_spending_is_totalled_separately() {
        let r = nov(
            &[budget("food", 6_000)],
            &[
                expense("food", date!(2026 - 11 - 3), 500),
                expense("games", date!(2026 - 11 - 4), 900),
                expense("pets", date!(2026 - 11 - 6), 350),
                item("pets", date!(2026 - 11 - 7), 50, SpendingKind::Refund),
            ],
        )
        .unwrap();
        // 900 + 350 - 50 = 1,200
        assert_eq!(r.unbudgeted_spent, thb(1_200));
        assert_eq!(r.total_spent, thb(500));
        assert_eq!(r.categories.len(), 1);
    }

    #[test]
    fn several_categories_sorted_with_totals() {
        let r = nov(
            &[
                budget("transport", 1_500),
                budget("food", 6_000),
                budget("coffee", 0),
            ],
            &[
                expense("food", date!(2026 - 11 - 3), 2_000),
                expense("transport", date!(2026 - 11 - 3), 1_600),
                expense("coffee", date!(2026 - 11 - 4), 65),
            ],
        )
        .unwrap();
        let names: Vec<_> = r.categories.iter().map(|c| c.category).collect();
        assert_eq!(names, ["coffee", "food", "transport"]);
        // zero limit: no utilisation, any spending is over budget
        assert_eq!(r.categories[0].utilisation_bp, None);
        assert!(r.categories[0].over_budget);
        // transport: 1,600 of 1,500
        assert_eq!(r.categories[2].remaining, thb(-100));
        // totals: limit 0 + 6,000 + 1,500 = 7,500; spent 65 + 2,000 + 1,600 = 3,665
        assert_eq!(r.total_limit, thb(7_500));
        assert_eq!(r.total_spent, thb(3_665));
        assert_eq!(r.total_remaining, thb(3_835));
    }

    #[test]
    fn budget_without_spending() {
        let r = nov(&[budget("gifts", 2_000)], &[]).unwrap();
        let c = &r.categories[0];
        assert_eq!(c.spent, thb(0));
        assert_eq!(c.remaining, thb(2_000));
        assert_eq!(c.utilisation_bp, Some(0));
        assert!(!c.over_budget);
    }

    #[test]
    fn utilisation_rounds_down() {
        let r = nov(
            &[budget("food", 3_000)],
            &[expense("food", date!(2026 - 11 - 3), 1_000)],
        )
        .unwrap();
        // 1,000 / 3,000 = 3,333.33 bp → 3,333
        assert_eq!(r.categories[0].utilisation_bp, Some(3_333));
    }

    #[test]
    fn rejects_duplicate_category() {
        assert_eq!(
            nov(&[budget("food", 1_000), budget("food", 2_000)], &[]),
            Err(BudgetError::DuplicateCategory)
        );
    }

    #[test]
    fn rejects_negative_limit() {
        assert_eq!(
            nov(&[budget("food", -1)], &[]),
            Err(BudgetError::NegativeLimit)
        );
    }

    #[test]
    fn rejects_non_positive_amounts() {
        for baht in [0, -10] {
            assert_eq!(
                nov(
                    &[budget("food", 1_000)],
                    &[expense("food", date!(2026 - 11 - 3), baht)]
                ),
                Err(BudgetError::NonPositiveAmount)
            );
        }
    }

    #[test]
    fn rejects_mixed_currencies() {
        let usd = Money::new(1_000, Currency::Usd);
        let mismatch = Err(BudgetError::Money(MoneyError::CurrencyMismatch {
            left: Currency::Thb,
            right: Currency::Usd,
        }));
        assert_eq!(
            nov(
                &[Budget {
                    category: "food",
                    limit: usd
                }],
                &[]
            ),
            mismatch
        );
        assert_eq!(
            nov(
                &[budget("food", 1_000)],
                &[SpendingItem {
                    category: "food",
                    date: date!(2026 - 10 - 3), // outside the month, still checked
                    amount: usd,
                    kind: SpendingKind::Expense,
                }]
            ),
            mismatch
        );
    }

    #[test]
    fn error_display() {
        assert_eq!(
            BudgetError::DuplicateCategory.to_string(),
            "category is budgeted more than once"
        );
        assert_eq!(
            BudgetError::Money(MoneyError::CurrencyMismatch {
                left: Currency::Thb,
                right: Currency::Usd
            })
            .to_string(),
            "cannot combine THB with USD"
        );
    }
}
