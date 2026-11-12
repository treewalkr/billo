//! Debt payoff planner: how long until debt-free, and how much interest, for a monthly
//! budget and a strategy.
//!
//! This is a **projection**, not a statement: each debt's interest is simplified to
//! `balance × annual rate / 12` per month (rounded with the given [`Rounding`]).
//!
//! Each month: add interest → pay every minimum → spend what is left of the budget on
//! the strategy's target, rolling over to the next target when one is cleared.

use std::fmt;

use crate::card::MinPaymentRule;
use crate::money::{Currency, Money, MoneyError};
use crate::rounding::Rounding;

/// Simulation stops after this many months (50 years).
pub const MAX_MONTHS: u16 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Smallest balance first.
    Snowball,
    /// Highest interest rate first.
    Avalanche,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Minimum {
    /// A fixed installment (loans).
    Fixed(Money),
    /// A card's percentage-or-floor rule, applied to each month's balance.
    Rule(MinPaymentRule),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Debt<Id> {
    pub id: Id,
    pub balance: Money,
    pub annual_rate_bp: u32,
    pub minimum: Minimum,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebtOutcome<Id> {
    pub id: Id,
    /// 1-based month in which the debt reaches zero.
    pub paid_off_month: u16,
    pub interest: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutcome<Id> {
    pub strategy: Strategy,
    /// Months until every debt is cleared.
    pub months: u16,
    pub total_interest: Money,
    pub total_paid: Money,
    /// In payoff order.
    pub debts: Vec<DebtOutcome<Id>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    NoDebts,
    /// The budget does not cover this month's minimum payments.
    BudgetBelowMinimums {
        month: u16,
        minimums: Money,
    },
    /// Not debt-free within [`MAX_MONTHS`].
    NeverPaysOff,
    NegativeBalance,
    Money(MoneyError),
}

impl From<MoneyError> for PlanError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDebts => f.write_str("no debts to plan"),
            Self::BudgetBelowMinimums { month, minimums } => {
                write!(
                    f,
                    "budget does not cover minimum payments of {minimums} in month {month}"
                )
            }
            Self::NeverPaysOff => write!(f, "not debt-free within {MAX_MONTHS} months"),
            Self::NegativeBalance => f.write_str("debt balance cannot be negative"),
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PlanError {}

struct Live<'a, Id> {
    debt: &'a Debt<Id>,
    balance: i64,
    interest: i64,
    done: Option<u16>,
}

/// Simulate paying `debts` with `budget` per month using `strategy`.
pub fn plan<Id: Clone>(
    debts: &[Debt<Id>],
    budget: Money,
    strategy: Strategy,
    rounding: Rounding,
) -> Result<PlanOutcome<Id>, PlanError> {
    let currency = budget.currency();
    if debts.is_empty() {
        return Err(PlanError::NoDebts);
    }
    for d in debts {
        same(currency, d.balance)?;
        if let Minimum::Fixed(m) = d.minimum {
            same(currency, m)?;
        }
        if d.balance.is_negative() {
            return Err(PlanError::NegativeBalance);
        }
    }

    let mut live: Vec<Live<'_, Id>> = debts
        .iter()
        .map(|d| Live {
            debt: d,
            balance: d.balance.minor(),
            interest: 0,
            done: None,
        })
        .collect();
    for l in &mut live {
        if l.balance == 0 {
            l.done = Some(0);
        }
    }
    let mut total_paid: i64 = 0;
    let mut month: u16 = 0;

    while live.iter().any(|l| l.done.is_none()) {
        month += 1;
        if month > MAX_MONTHS {
            return Err(PlanError::NeverPaysOff);
        }

        // 1. Interest.
        for l in live.iter_mut().filter(|l| l.done.is_none()) {
            let i = rounding
                .apply(
                    i128::from(l.balance) * i128::from(l.debt.annual_rate_bp),
                    10_000 * 12,
                )
                .ok_or(MoneyError::Overflow)?;
            l.balance = l.balance.checked_add(i).ok_or(MoneyError::Overflow)?;
            l.interest += i;
        }

        // 2. Minimums.
        let mut mins = Vec::with_capacity(live.len());
        for l in &live {
            let due = if l.done.is_some() {
                0
            } else {
                let m = match l.debt.minimum {
                    Minimum::Fixed(m) => m.minor(),
                    Minimum::Rule(r) => r
                        .minimum_due(Money::new(l.balance, currency), Money::zero(currency))
                        .map_err(|_| MoneyError::Overflow)?
                        .minor(),
                };
                m.clamp(0, l.balance)
            };
            mins.push(due);
        }
        let min_total: i64 = mins.iter().sum();
        if min_total > budget.minor() {
            return Err(PlanError::BudgetBelowMinimums {
                month,
                minimums: Money::new(min_total, currency),
            });
        }
        for (l, m) in live.iter_mut().zip(&mins) {
            l.balance -= m;
        }
        let mut extra = budget.minor() - min_total;
        total_paid += min_total;

        // 3. Extra to targets, rolling over.
        while extra > 0 {
            let Some(t) = target(&live, strategy) else {
                break;
            };
            let pay = extra.min(live[t].balance);
            live[t].balance -= pay;
            extra -= pay;
            total_paid += pay;
            if live[t].balance == 0 {
                live[t].done = Some(month);
            }
        }
        for l in live
            .iter_mut()
            .filter(|l| l.done.is_none() && l.balance == 0)
        {
            l.done = Some(month);
        }

        if live.iter().all(|l| l.done.is_none()) && extra == 0 && min_total == 0 {
            return Err(PlanError::NeverPaysOff);
        }
    }

    let mut outcomes: Vec<DebtOutcome<Id>> = live
        .iter()
        .map(|l| DebtOutcome {
            id: l.debt.id.clone(),
            paid_off_month: l.done.unwrap_or(0),
            interest: Money::new(l.interest, currency),
        })
        .collect();
    outcomes.sort_by_key(|o| o.paid_off_month);
    let total_interest = live.iter().map(|l| l.interest).sum();
    Ok(PlanOutcome {
        strategy,
        months: month,
        total_interest: Money::new(total_interest, currency),
        total_paid: Money::new(total_paid, currency),
        debts: outcomes,
    })
}

fn target<Id>(live: &[Live<'_, Id>], strategy: Strategy) -> Option<usize> {
    let open = live
        .iter()
        .enumerate()
        .filter(|(_, l)| l.done.is_none() && l.balance > 0);
    match strategy {
        Strategy::Snowball => open.min_by_key(|(i, l)| (l.balance, *i)),
        Strategy::Avalanche => {
            open.min_by_key(|(i, l)| (std::cmp::Reverse(l.debt.annual_rate_bp), l.balance, *i))
        }
    }
    .map(|(i, _)| i)
}

fn same(expected: Currency, m: Money) -> Result<(), MoneyError> {
    if m.currency() == expected {
        Ok(())
    } else {
        Err(MoneyError::CurrencyMismatch {
            left: expected,
            right: m.currency(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn thb(baht: i64) -> Money {
        Money::new(baht * 100, Currency::Thb)
    }

    fn debt(id: &'static str, baht: i64, rate_bp: u32, min_baht: i64) -> Debt<&'static str> {
        Debt {
            id,
            balance: thb(baht),
            annual_rate_bp: rate_bp,
            minimum: Minimum::Fixed(thb(min_baht)),
        }
    }

    fn run(
        debts: &[Debt<&'static str>],
        budget: i64,
        s: Strategy,
    ) -> Result<PlanOutcome<&'static str>, PlanError> {
        plan(debts, thb(budget), s, Rounding::default())
    }

    #[test]
    fn zero_interest_single_debt() {
        let out = run(&[debt("a", 1_000, 0, 300)], 500, Strategy::Avalanche).unwrap();
        assert_eq!(out.months, 2);
        assert_eq!(out.total_interest, thb(0));
        assert_eq!(out.total_paid, thb(1_000));
    }

    #[test]
    fn one_percent_a_month() {
        // ฿10,000 at 12%: month 1 interest ฿100, pay ฿5,100 → ฿5,000; month 2 ฿50, pay ฿5,050.
        let out = run(&[debt("a", 10_000, 1_200, 100)], 5_100, Strategy::Avalanche).unwrap();
        assert_eq!(out.months, 2);
        assert_eq!(out.total_interest, thb(150));
        assert_eq!(out.total_paid, thb(10_150));
    }

    #[test]
    fn snowball_clears_smallest_first_avalanche_saves_interest() {
        let debts = [
            debt("card", 50_000, 1_600, 2_000),
            debt("cash", 30_000, 2_500, 1_000),
            debt("small", 3_000, 1_200, 500),
        ];
        let snow = run(&debts, 10_000, Strategy::Snowball).unwrap();
        let aval = run(&debts, 10_000, Strategy::Avalanche).unwrap();
        assert_eq!(snow.debts[0].id, "small");
        assert_eq!(aval.debts[0].id, "cash");
        assert!(aval.total_interest.minor() <= snow.total_interest.minor());
        for out in [&snow, &aval] {
            let principal = thb(83_000).minor();
            assert_eq!(
                out.total_paid.minor(),
                principal + out.total_interest.minor()
            );
        }
    }

    #[test]
    fn freed_minimums_roll_over() {
        // Without rollover the ฿10,000 debt would take 10 months at ฿1,000.
        let debts = [debt("a", 1_000, 0, 500), debt("b", 10_000, 0, 500)];
        let out = run(&debts, 2_000, Strategy::Snowball).unwrap();
        assert_eq!(out.months, 6); // ฿11,000 / ฿2,000 → 5.5
        assert_eq!(out.debts[0].paid_off_month, 1);
    }

    #[test]
    fn rule_based_minimum() {
        let rule = MinPaymentRule::new(1_000, thb(500), Rounding::default()).unwrap();
        let d = [Debt {
            id: "card",
            balance: thb(5_000),
            annual_rate_bp: 0,
            minimum: Minimum::Rule(rule),
        }];
        // Budget equals the floor: ฿500 a month → 10 months.
        assert_eq!(run(&d, 500, Strategy::Avalanche).unwrap().months, 10);
    }

    #[test]
    fn budget_below_minimums() {
        let err = run(&[debt("a", 10_000, 0, 1_000)], 900, Strategy::Snowball).unwrap_err();
        assert_eq!(
            err,
            PlanError::BudgetBelowMinimums {
                month: 1,
                minimums: thb(1_000)
            }
        );
    }

    #[test]
    fn interest_outgrows_budget() {
        // 25% on ฿1,000,000 is ~฿20,833 a month; ฿10,000 never catches up.
        let err = run(
            &[debt("a", 1_000_000, 2_500, 0)],
            10_000,
            Strategy::Snowball,
        )
        .unwrap_err();
        assert_eq!(err, PlanError::NeverPaysOff);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert_eq!(run(&[], 1, Strategy::Snowball), Err(PlanError::NoDebts));
        assert_eq!(
            run(&[debt("a", -1, 0, 0)], 1, Strategy::Snowball),
            Err(PlanError::NegativeBalance)
        );
        let usd = Debt {
            balance: Money::new(100, Currency::Usd),
            ..debt("a", 1, 0, 0)
        };
        assert!(matches!(
            run(&[usd], 1, Strategy::Snowball),
            Err(PlanError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }
}
