//! Monthly budgets: turns stored transactions and budgets into
//! [`billo_domain::budget::monthly_budget`] input.
//!
//! What counts as spending: expenses and fees add to their category; refunds (income
//! on a card, or income filed under a spending category) reduce it; other income,
//! payments and transfers are ignored. Only amounts in the budget currency count.

use billo_api::{
    AccountData, AccountKind, BudgetData, CategoryData, CategoryKind, Record, TransactionData,
    TransactionKind,
};
use billo_domain::budget::{Budget, MonthlyBudget, SpendingItem, SpendingKind, monthly_budget};
use billo_domain::{Currency, Money};
use time::{Date, Month};
use uuid::Uuid;

use crate::forms::is_card_like;

/// Budgets and spending are in baht for now.
pub const CURRENCY: Currency = Currency::Thb;

/// A category key for the domain: `None` is "no category" (never budgeted).
pub type Key = Option<Uuid>;

/// The first and last day of the month.
pub fn bounds(year: i32, month: Month) -> Option<(Date, Date)> {
    let first = Date::from_calendar_date(year, month, 1).ok()?;
    let last = Date::from_calendar_date(year, month, month.length(year)).ok()?;
    Some((first, last))
}

/// The month before.
pub const fn previous(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::January => (year - 1, Month::December),
        m => (year, m.previous()),
    }
}

/// The month after.
pub const fn next(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::December => (year + 1, Month::January),
        m => (year, m.next()),
    }
}

/// How a transaction counts towards budgets, if at all.
pub fn spending_kind(
    account: Option<AccountKind>,
    category: Option<CategoryKind>,
    kind: TransactionKind,
) -> Option<SpendingKind> {
    match kind {
        TransactionKind::Expense | TransactionKind::Fee => Some(SpendingKind::Expense),
        TransactionKind::Income
            if account.is_some_and(is_card_like) || category == Some(CategoryKind::Expense) =>
        {
            Some(SpendingKind::Refund)
        }
        TransactionKind::Income => Some(SpendingKind::Income),
        TransactionKind::Payment | TransactionKind::Transfer | TransactionKind::InterestAdjust => {
            None
        }
    }
}

/// Budget vs. spending for `year`/`month`. Budgets for other months or for deleted
/// categories are left out, as are transactions in other currencies.
pub fn month(
    year: i32,
    month: Month,
    accounts: &[Record<AccountData>],
    categories: &[Record<CategoryData>],
    txns: &[Record<TransactionData>],
    budgets: &[Record<BudgetData>],
) -> Result<MonthlyBudget<Key>, String> {
    let first = bounds(year, month).map(|(f, _)| f);
    let live = |id: Uuid| categories.iter().find(|c| c.id == id);
    let budgets: Vec<Budget<Key>> = budgets
        .iter()
        .filter(|b| Some(b.data.period_month) == first && live(b.data.category_id).is_some())
        .filter(|b| b.data.limit.currency == CURRENCY.code())
        .map(|b| Budget {
            category: Some(b.data.category_id),
            limit: Money::new(b.data.limit.minor, CURRENCY),
        })
        .collect();
    let items: Vec<SpendingItem<Key>> = txns
        .iter()
        .filter(|t| t.data.amount.currency == CURRENCY.code() && t.data.amount.minor > 0)
        .filter_map(|t| {
            let account = accounts
                .iter()
                .find(|a| a.id == t.data.account_id)
                .map(|a| a.data.kind);
            let category = t.data.category_id.and_then(live);
            let kind = spending_kind(account, category.map(|c| c.data.kind), t.data.kind)?;
            Some(SpendingItem {
                category: category.map(|c| c.id),
                date: t.data.occurred_on,
                amount: Money::new(t.data.amount.minor, CURRENCY),
                kind,
            })
        })
        .collect();
    monthly_budget(year, month, CURRENCY, &budgets, &items).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use time::macros::date;

    fn rec<D>(n: u128, data: D) -> Record<D> {
        Record {
            id: Uuid::from_u128(n),
            version: 1,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            data,
        }
    }

    fn account(n: u128, kind: AccountKind) -> Record<AccountData> {
        rec(
            n,
            AccountData {
                kind,
                name: "a".into(),
                currency: "THB".into(),
                rules: billo_api::rules::AccountRules::default(),
                opened_on: None,
                closed_on: None,
            },
        )
    }

    fn category(n: u128, kind: CategoryKind) -> Record<CategoryData> {
        rec(
            n,
            CategoryData {
                parent_id: None,
                name_th: None,
                name_en: Some(format!("c{n}")),
                kind,
            },
        )
    }

    fn txn(
        n: u128,
        account: u128,
        category: Option<u128>,
        kind: TransactionKind,
        baht: i64,
        on: Date,
    ) -> Record<TransactionData> {
        rec(
            n,
            TransactionData {
                account_id: Uuid::from_u128(account),
                category_id: category.map(Uuid::from_u128),
                occurred_on: on,
                posted_on: None,
                amount: billo_api::Money::new(baht * 100, "THB"),
                orig_amount: None,
                kind,
                transfer_peer_id: None,
                installment_plan_id: None,
                note: None,
            },
        )
    }

    fn budget(n: u128, category: u128, month: Date, baht: i64) -> Record<BudgetData> {
        rec(
            n,
            BudgetData {
                category_id: Uuid::from_u128(category),
                period_month: month,
                limit: billo_api::Money::new(baht * 100, "THB"),
            },
        )
    }

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, CURRENCY)
    }

    #[test]
    fn month_navigation() {
        assert_eq!(previous(2026, Month::January), (2025, Month::December));
        assert_eq!(next(2026, Month::December), (2027, Month::January));
        assert_eq!(next(2026, Month::March), (2026, Month::April));
        assert_eq!(
            bounds(2028, Month::February),
            Some((date!(2028 - 02 - 01), date!(2028 - 02 - 29)))
        );
    }

    #[test]
    fn what_counts_as_spending() {
        use TransactionKind as T;
        let card = Some(AccountKind::Card);
        let bank = Some(AccountKind::Bank);
        let exp = Some(CategoryKind::Expense);
        let inc = Some(CategoryKind::Income);
        assert_eq!(
            spending_kind(bank, exp, T::Expense),
            Some(SpendingKind::Expense)
        );
        assert_eq!(
            spending_kind(card, None, T::Fee),
            Some(SpendingKind::Expense)
        );
        assert_eq!(
            spending_kind(card, exp, T::Income),
            Some(SpendingKind::Refund)
        );
        assert_eq!(
            spending_kind(bank, exp, T::Income),
            Some(SpendingKind::Refund)
        );
        assert_eq!(
            spending_kind(bank, inc, T::Income),
            Some(SpendingKind::Income)
        );
        assert_eq!(spending_kind(card, None, T::Payment), None);
        assert_eq!(spending_kind(bank, None, T::Transfer), None);
    }

    #[test]
    fn builds_the_month() {
        let nov = date!(2026 - 11 - 01);
        let accounts = [account(1, AccountKind::Card), account(2, AccountKind::Bank)];
        let cats = [
            category(10, CategoryKind::Expense), // food
            category(11, CategoryKind::Expense), // shopping (no budget)
            category(12, CategoryKind::Income),  // salary
        ];
        let txns = [
            txn(
                100,
                1,
                Some(10),
                TransactionKind::Expense,
                1_200,
                date!(2026 - 11 - 3),
            ),
            txn(
                101,
                1,
                Some(10),
                TransactionKind::Income,
                200,
                date!(2026 - 11 - 4),
            ), // refund
            txn(
                102,
                2,
                Some(10),
                TransactionKind::Expense,
                300,
                date!(2026 - 10 - 31),
            ), // october
            txn(
                103,
                2,
                Some(11),
                TransactionKind::Expense,
                450,
                date!(2026 - 11 - 5),
            ),
            txn(
                104,
                2,
                None,
                TransactionKind::Expense,
                50,
                date!(2026 - 11 - 6),
            ),
            txn(
                105,
                2,
                Some(12),
                TransactionKind::Income,
                40_000,
                date!(2026 - 11 - 25),
            ),
            txn(
                106,
                1,
                None,
                TransactionKind::Payment,
                5_000,
                date!(2026 - 11 - 20),
            ),
            txn(
                107,
                2,
                Some(99),
                TransactionKind::Expense,
                70,
                date!(2026 - 11 - 7),
            ), // deleted category
        ];
        let budgets = [
            budget(200, 10, nov, 1_500),
            budget(201, 10, date!(2026 - 10 - 01), 9_999), // october
            budget(202, 99, nov, 500),                     // deleted category
        ];
        let r = month(2026, Month::November, &accounts, &cats, &txns, &budgets).unwrap();
        assert_eq!(r.categories.len(), 1);
        let food = &r.categories[0];
        // 1,200 - 200 = 1,000 of 1,500 → 500 left, 66.66% → 6,666 bp
        assert_eq!(food.spent, thb(1_000));
        assert_eq!(food.remaining, thb(500));
        assert_eq!(food.utilisation_bp, Some(6_666));
        // 450 + 50 + 70 = 570
        assert_eq!(r.unbudgeted_spent, thb(570));
    }
}
