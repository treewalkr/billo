//! SAMPLE DATA for the spike. Not a real card, not user data: a made-up card with
//! made-up purchases, so the screen has something computed by `billo-domain` to show.
//! Every real card rule will be a user setting.

use billo_domain::card::{
    CardRules, CycleRules, DueRule, DueShift, Entry, EntryKind, InstallmentPlan, InterestRules,
    InterestStart, MinPaymentRule, Remainder, Statement, StatementError,
};
use billo_domain::{Currency, DayCount, Money, Rounding, WeekendCalendar};
use time::macros::date;
use time::{Date, Month};

pub const CARD_NAME: &str = "Sample card";

/// What the "This month" screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoStatement {
    pub statement: Statement,
    pub installment_name: &'static str,
    pub installment_portion: Money,
    pub installment_months: u16,
}

const fn thb(baht: i64) -> Money {
    Money::new(baht * 100, Currency::Thb)
}

fn charge(tx: Date, posted: Date, amount: Money) -> Entry {
    Entry {
        kind: EntryKind::Charge,
        transaction_date: tx,
        posting_date: posted,
        amount,
    }
}

fn credit(on: Date, amount: Money) -> Entry {
    Entry {
        kind: EntryKind::Credit,
        transaction_date: on,
        posting_date: on,
        amount,
    }
}

/// Sample card: cut-off on the 25th, due 20 days later (next business day), 8% or ฿1,000
/// minimum, 16% a year interest with a grace period.
fn rules() -> CardRules {
    CardRules {
        cycle: CycleRules::new(25, DueRule::DaysAfterStatement(20), DueShift::Next)
            .expect("sample cycle is valid"),
        min_payment: MinPaymentRule::new(800, thb(1_000), Rounding::default())
            .expect("sample minimum is valid"),
        interest: InterestRules::new(
            1_600,
            DayCount::Actual365,
            InterestStart::TransactionDate,
            Rounding::default(),
            true,
        )
        .expect("sample interest is valid"),
    }
}

/// The November 2026 statement: October was only partly paid, so November loses the
/// grace period and charges interest; a 0% installment plan adds a portion due in full.
pub fn november() -> Result<DemoStatement, StatementError> {
    let rules = rules();
    let cal = WeekendCalendar::new();

    let plan = InstallmentPlan::new(
        thb(24_990),
        10,
        0,
        date!(2026 - 11 - 10),
        Rounding::default(),
        Remainder::Last,
    )
    .expect("sample plan is valid");

    let mut entries = vec![
        // October cycle (26 Sep – 25 Oct).
        charge(
            date!(2026 - 10 - 3),
            date!(2026 - 10 - 4),
            Money::new(1_245_050, Currency::Thb),
        ),
        charge(
            date!(2026 - 10 - 18),
            date!(2026 - 10 - 19),
            Money::new(389_000, Currency::Thb),
        ),
        // Part payment before October's due date.
        credit(date!(2026 - 11 - 5), thb(5_000)),
        // November cycle (26 Oct – 25 Nov).
        charge(
            date!(2026 - 10 - 30),
            date!(2026 - 10 - 31),
            Money::new(214_975, Currency::Thb),
        ),
        charge(
            date!(2026 - 11 - 8),
            date!(2026 - 11 - 9),
            Money::new(67_800, Currency::Thb),
        ),
    ];
    entries.extend(plan.entries());

    let zero = Money::zero(Currency::Thb);
    let october = rules.statement(2026, Month::October, None, &entries, zero, &cal)?;
    let nov_period = rules.cycle.period_for_month(2026, Month::November, &cal);
    let statement = rules.statement(
        2026,
        Month::November,
        Some(&october.as_previous()),
        &entries,
        plan.due_in_period(&nov_period),
        &cal,
    )?;

    Ok(DemoStatement {
        statement,
        installment_name: "Phone, 0% for 10 months",
        installment_portion: plan.due_in_period(&nov_period),
        installment_months: plan.months(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn november_statement_is_consistent() {
        let d = november().unwrap();
        let s = d.statement;
        assert_eq!(s.period.statement_date, date!(2026 - 11 - 25));
        // 25 Nov + 20 days = 15 Dec (a Tuesday), no shift.
        assert_eq!(s.period.due_date, date!(2026 - 12 - 15));
        assert!(!s.previous_paid_in_full);
        assert!(s.interest.minor() > 0);
        assert_eq!(d.installment_portion, Money::new(249_900, Currency::Thb));
        assert_eq!(
            s.closing_balance.minor(),
            s.opening_balance.minor() + s.charges.minor() - s.credits.minor() + s.interest.minor()
        );
        assert!(s.minimum_due.minor() >= d.installment_portion.minor());
        assert!(s.minimum_due.minor() <= s.closing_balance.minor());
    }
}
