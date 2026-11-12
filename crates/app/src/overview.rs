//! "This month": what's due on each card and loan, built from stored records with
//! `billo-domain`. Pure: everything, including today, is passed in.
//!
//! Cards: statements are chained from the card's first activity up to the cycle that
//! contains today, so balances, lost grace periods and overdue minimums carry over.
//! The last statement already cut is the bill to pay; the open cycle is shown as
//! "so far". Loans: the first scheduled payment from the start of this month.

use billo_api::{
    AccountData, AccountKind, InstallmentPlanData, Record, TransactionData, TransactionKind,
};
use billo_domain::card::{
    CardRules, Entry, EntryKind, InstallmentPlan, Statement, StatementPeriod,
};
use billo_domain::{BusinessCalendar, Currency, LoanPayment, Money, PaymentKind};
use time::{Date, Duration};
use uuid::Uuid;

use crate::convert::{self, Invalid};

/// Safety limit on statement chaining (50 years of monthly statements).
const MAX_STATEMENTS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing owed on this statement.
    NothingDue,
    PaidInFull,
    MinimumPaid,
    Unpaid,
    /// Past the due date with less than the minimum paid.
    Overdue,
    /// The statement isn't cut yet; amounts are so far.
    Open,
    /// A loan payment that's been made.
    Paid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardDue {
    pub statement: Statement,
    /// Installment portions on this statement (part of the minimum, due in full).
    pub installments: Money,
    /// Payments and refunds posted since the statement date.
    pub paid: Money,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanDue {
    pub payment: LoanPayment,
    /// 1-based number of this payment, out of `of`.
    pub number: usize,
    pub of: usize,
    /// Payments recorded for this installment.
    pub paid: Money,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    Card(CardDue),
    Loan(LoanDue),
}

/// One thing due, for one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub account_id: Uuid,
    pub account_name: String,
    pub due_date: Date,
    pub detail: Detail,
}

/// An account whose data couldn't be read; shown in place of its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemError {
    pub account_id: Uuid,
    pub account_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThisMonth {
    /// Sorted by due date.
    pub items: Vec<Item>,
    pub errors: Vec<ItemError>,
}

/// Builds "This month" for all accounts. One account's bad data becomes an
/// [`ItemError`] and never hides the others.
pub fn this_month(
    accounts: &[Record<AccountData>],
    transactions: &[Record<TransactionData>],
    plans: &[Record<InstallmentPlanData>],
    today: Date,
    cal: &impl BusinessCalendar,
) -> ThisMonth {
    let mut out = ThisMonth::default();
    for a in accounts {
        let txns: Vec<TransactionData> = transactions
            .iter()
            .filter(|t| t.data.account_id == a.id)
            .map(|t| t.data.clone())
            .collect();
        let result = match a.data.kind {
            AccountKind::Card | AccountKind::Revolving => {
                let card_plans: Vec<InstallmentPlanData> = plans
                    .iter()
                    .filter(|p| p.data.card_account_id == a.id)
                    .map(|p| p.data.clone())
                    .collect();
                card_due(&a.data, &txns, &card_plans, today, cal).map(|dues| {
                    dues.into_iter()
                        .map(|d| (d.statement.period.due_date, Detail::Card(d)))
                        .collect::<Vec<_>>()
                })
            }
            AccountKind::Loan => loan_due(&a.data, &txns, today).map(|d| {
                d.into_iter()
                    .map(|d| (d.payment.date, Detail::Loan(d)))
                    .collect()
            }),
            AccountKind::Cash | AccountKind::Bank => Ok(Vec::new()),
        };
        match result {
            Ok(list) => out
                .items
                .extend(list.into_iter().map(|(due_date, detail)| Item {
                    account_id: a.id,
                    account_name: a.data.name.clone(),
                    due_date,
                    detail,
                })),
            Err(e) => out.errors.push(ItemError {
                account_id: a.id,
                account_name: a.data.name.clone(),
                message: e.0,
            }),
        }
    }
    out.items.sort_by(|a, b| {
        a.due_date
            .cmp(&b.due_date)
            .then_with(|| a.account_name.cmp(&b.account_name))
    });
    out
}

/// Plans as domain values, keeping only portions up to an early close.
struct Plan {
    plan: InstallmentPlan,
    closed_on: Option<Date>,
}

impl Plan {
    fn keeps(&self, date: Date) -> bool {
        self.closed_on.is_none_or(|c| date <= c)
    }

    fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.plan
            .entries()
            .into_iter()
            .filter(|e| self.keeps(e.posting_date))
    }

    fn due_in(&self, period: &StatementPeriod) -> i64 {
        self.plan
            .schedule()
            .iter()
            .filter(|p| period.contains(p.posting_date) && self.keeps(p.posting_date))
            .map(|p| p.amount().minor())
            .sum()
    }
}

/// Every statement from the card's first activity through the cycle containing
/// `today` (the last one is still open).
pub fn card_statements(
    rules: &CardRules,
    currency: Currency,
    entries: &[Entry],
    due_in_full: impl Fn(&StatementPeriod) -> Result<Money, Invalid>,
    today: Date,
    cal: &impl BusinessCalendar,
) -> Result<Vec<Statement>, Invalid> {
    let first = entries
        .iter()
        .map(|e| e.posting_date.min(e.transaction_date))
        .min()
        .map_or(today, |d| d.min(today));
    let mut period = rules.cycle.period_containing(first, cal);
    let mut previous = None;
    let mut out = Vec::new();
    loop {
        if out.len() >= MAX_STATEMENTS {
            return Err(Invalid("too much history to build statements".into()));
        }
        let due = due_in_full(&period)?;
        if due.currency() != currency {
            return Err(Invalid("an installment plan is in another currency".into()));
        }
        let year = period.statement_date.year();
        let month = period.statement_date.month();
        let s = rules
            .statement(year, month, previous.as_ref(), entries, due, cal)
            .map_err(|e| Invalid(format!("statement for {month} {year}: {e}")))?;
        let done = s.period.statement_date >= today;
        previous = Some(s.as_previous());
        out.push(s);
        if done {
            return Ok(out);
        }
        let next_day = period.statement_date + Duration::DAY;
        period = rules.cycle.period_containing(next_day, cal);
    }
}

/// The last statement already cut (if any) and the open cycle, for one card.
pub fn card_due(
    account: &AccountData,
    txns: &[TransactionData],
    plans: &[InstallmentPlanData],
    today: Date,
    cal: &impl BusinessCalendar,
) -> Result<Vec<CardDue>, Invalid> {
    let currency = convert::currency(&account.currency)?;
    let settings =
        account.rules.card.as_ref().ok_or_else(|| {
            Invalid("card settings are missing; edit the card to add them".into())
        })?;
    let (rules, _limit) = convert::card_rules(settings, currency)?;

    let plans = plans
        .iter()
        .map(|p| {
            let plan = convert::installment_plan(p)?;
            if plan.principal().currency() != currency {
                return Err(Invalid("an installment plan is in another currency".into()));
            }
            Ok(Plan {
                plan,
                closed_on: p.closed_on,
            })
        })
        .collect::<Result<Vec<_>, Invalid>>()?;

    let mut entries = Vec::new();
    for t in txns {
        if let Some(e) = convert::card_entry(t, currency)? {
            entries.push(e);
        }
    }
    entries.extend(plans.iter().flat_map(Plan::entries));

    let due_in_full = |period: &StatementPeriod| -> Result<Money, Invalid> {
        let mut total: i64 = 0;
        for p in &plans {
            total = total
                .checked_add(p.due_in(period))
                .ok_or_else(|| Invalid("installment amounts are too large".into()))?;
        }
        Ok(Money::new(total, currency))
    };
    let statements = card_statements(&rules, currency, &entries, due_in_full, today, cal)?;

    let paid_between = |after: Date, until: Date| -> Money {
        let minor = entries
            .iter()
            .filter(|e| {
                e.kind == EntryKind::Credit && e.posting_date > after && e.posting_date <= until
            })
            .map(|e| e.amount.minor())
            .sum();
        Money::new(minor, currency)
    };

    let mut out = Vec::new();
    let Some((&open, earlier)) = statements.split_last() else {
        return Err(Invalid("no statements".into()));
    };
    if let Some(&cut) = earlier.last() {
        let paid = paid_between(cut.period.statement_date, open.period.statement_date);
        let closing = cut.closing_balance.minor();
        let status = if closing <= 0 {
            Status::NothingDue
        } else if paid.minor() >= closing {
            Status::PaidInFull
        } else if today > cut.period.due_date && paid.minor() < cut.minimum_due.minor() {
            Status::Overdue
        } else if paid.minor() >= cut.minimum_due.minor() {
            Status::MinimumPaid
        } else {
            Status::Unpaid
        };
        out.push(CardDue {
            statement: cut,
            installments: due_in_full(&cut.period)?,
            paid,
            status,
        });
    }
    out.push(CardDue {
        statement: open,
        installments: due_in_full(&open.period)?,
        paid: paid_between(open.period.statement_date, open.period.due_date),
        status: if open.closing_balance.minor() <= 0 {
            Status::NothingDue
        } else {
            Status::Open
        },
    });
    Ok(out)
}

/// The loan's first regular payment on or after the 1st of this month. `None` once
/// the schedule has ended.
pub fn loan_due(
    account: &AccountData,
    txns: &[TransactionData],
    today: Date,
) -> Result<Option<LoanDue>, Invalid> {
    let currency = convert::currency(&account.currency)?;
    let settings = account
        .rules
        .loan
        .as_ref()
        .ok_or_else(|| Invalid("loan terms are missing; edit the loan to add them".into()))?;
    let terms = convert::loan_terms(settings, currency)?;
    let schedule: Vec<LoanPayment> = terms
        .schedule(&[])
        .map_err(|e| Invalid(format!("Loan: {e}")))?
        .into_iter()
        .filter(|p| p.kind == PaymentKind::Regular)
        .collect();
    let month_start = today.replace_day(1).unwrap_or(today);
    let Some(i) = schedule.iter().position(|p| p.date >= month_start) else {
        return Ok(None);
    };
    let payment = schedule[i];
    let after = i.checked_sub(1).map(|j| schedule[j].date);
    let until = schedule.get(i + 1).map(|p| p.date);

    let mut paid: i64 = 0;
    for t in txns {
        if t.kind != TransactionKind::Payment {
            continue;
        }
        let m = convert::money(&t.amount)?;
        if m.currency() != currency {
            return Err(Invalid("a payment is in another currency".into()));
        }
        let in_window =
            after.is_none_or(|a| t.occurred_on > a) && until.is_none_or(|u| t.occurred_on < u);
        if in_window {
            paid = paid.saturating_add(m.minor());
        }
    }
    let status = if paid >= payment.amount.minor() {
        Status::Paid
    } else if today > payment.date {
        Status::Overdue
    } else {
        Status::Unpaid
    };
    Ok(Some(LoanDue {
        payment,
        number: i + 1,
        of: schedule.len(),
        paid: Money::new(paid, currency),
        status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::tests::{card_settings, plan, txn};
    use billo_api::rules::{AccountRules, DayCount, LoanMethod, LoanSettings, RoundingSettings};
    use billo_domain::WeekendCalendar;
    use time::OffsetDateTime;
    use time::macros::date;

    const TODAY: Date = date!(2026 - 11 - 12);

    fn thb(minor: i64) -> Money {
        Money::new(minor, Currency::Thb)
    }

    fn record<D>(id: u128, data: D) -> Record<D> {
        Record {
            id: Uuid::from_u128(id),
            version: 1,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            data,
        }
    }

    fn card() -> AccountData {
        let mut s = card_settings();
        s.credit_limit_minor = None;
        AccountData {
            kind: AccountKind::Card,
            name: "Card".into(),
            currency: "THB".into(),
            rules: AccountRules {
                card: Some(s),
                ..AccountRules::default()
            },
            opened_on: None,
            closed_on: None,
        }
    }

    fn posted(kind: TransactionKind, minor: i64, on: Date, posted: Date) -> TransactionData {
        TransactionData {
            posted_on: Some(posted),
            ..txn(kind, minor, on)
        }
    }

    /// The scenario the browser test checks by hand: cut-off 25th, due 20 days later
    /// (moved off weekends), 8% or ฿1,000, 16% a year from the purchase date, grace.
    fn scenario() -> (Vec<TransactionData>, Vec<InstallmentPlanData>) {
        let txns = vec![
            posted(
                TransactionKind::Expense,
                1_000_000,
                date!(2026 - 10 - 10),
                date!(2026 - 10 - 11),
            ),
            posted(
                TransactionKind::Expense,
                250_050,
                date!(2026 - 10 - 20),
                date!(2026 - 10 - 21),
            ),
            posted(
                TransactionKind::Expense,
                100_000,
                date!(2026 - 11 - 01),
                date!(2026 - 11 - 02),
            ),
            txn(TransactionKind::Payment, 350_000, date!(2026 - 11 - 05)),
        ];
        // ฿12,000 at 0% over 6 months, first portion 15 Oct.
        (txns, vec![plan(1_200_000, 6, 0)])
    }

    #[test]
    fn hand_checked_statements() {
        let (txns, plans) = scenario();
        let dues = card_due(&card(), &txns, &plans, TODAY, &WeekendCalendar::new()).unwrap();
        assert_eq!(dues.len(), 2);

        // October statement (26 Sep – 25 Oct): 10,000 + 2,500.50 + 2,000 portion.
        let oct = &dues[0];
        let s = oct.statement;
        assert_eq!(s.period.start, date!(2026 - 09 - 26));
        assert_eq!(s.period.statement_date, date!(2026 - 10 - 25));
        // 25 Oct + 20 = Sat 14 Nov → Mon 16 Nov.
        assert_eq!(s.period.due_date, date!(2026 - 11 - 16));
        assert_eq!(s.opening_balance, thb(0));
        assert_eq!(s.charges, thb(1_450_050));
        assert_eq!(s.credits, thb(0));
        assert_eq!(s.interest, thb(0));
        assert_eq!(s.closing_balance, thb(1_450_050));
        assert_eq!(oct.installments, thb(200_000));
        // 8% of (14,500.50 − 2,000) = 1,000.04 > 1,000 floor; plus the 2,000 portion.
        assert_eq!(s.minimum_due, thb(300_004));
        assert_eq!(oct.paid, thb(350_000));
        assert_eq!(oct.status, Status::MinimumPaid);

        // November so far (26 Oct – 25 Nov): October wasn't paid in full, so interest
        // accrues: 6 d × 14,500.50 + 4 d × 15,500.50 + 10 d × 12,000.50
        // + 11 d × 14,000.50 = 423,015.50 baht-days × 16% / 365 = 185.43.
        let nov = &dues[1];
        let s = nov.statement;
        assert_eq!(s.period.statement_date, date!(2026 - 11 - 25));
        assert_eq!(s.period.due_date, date!(2026 - 12 - 15));
        assert!(!s.previous_paid_in_full);
        assert_eq!(s.opening_balance, thb(1_450_050));
        assert_eq!(s.charges, thb(300_000));
        assert_eq!(s.credits, thb(350_000));
        assert_eq!(s.interest, thb(18_543));
        assert_eq!(s.closing_balance, thb(1_418_593));
        assert_eq!(nov.installments, thb(200_000));
        // 8% of (14,185.93 − 2,000) = 974.87 → floor 1,000; plus 2,000.
        assert_eq!(s.minimum_due, thb(300_000));
        assert_eq!(nov.status, Status::Open);
    }

    #[test]
    fn statuses_for_the_cut_statement() {
        let (mut txns, plans) = scenario();
        let status = |txns: &[TransactionData], today| {
            card_due(&card(), txns, &plans, today, &WeekendCalendar::new()).unwrap()[0].status
        };
        txns.pop();
        assert_eq!(status(&txns, TODAY), Status::Unpaid);
        assert_eq!(status(&txns, date!(2026 - 11 - 17)), Status::Overdue);
        txns.push(txn(
            TransactionKind::Payment,
            1_450_050,
            date!(2026 - 11 - 05),
        ));
        assert_eq!(status(&txns, TODAY), Status::PaidInFull);
    }

    #[test]
    fn new_card_has_only_the_open_cycle() {
        let dues = card_due(&card(), &[], &[], TODAY, &WeekendCalendar::new()).unwrap();
        assert_eq!(dues.len(), 1);
        assert_eq!(dues[0].statement.closing_balance, thb(0));
        assert_eq!(dues[0].status, Status::NothingDue);
    }

    #[test]
    fn plan_postings_are_not_double_counted() {
        let (mut txns, plans) = scenario();
        let mut posting = posted(
            TransactionKind::Expense,
            200_000,
            date!(2026 - 10 - 15),
            date!(2026 - 10 - 15),
        );
        posting.installment_plan_id = Some(Uuid::nil());
        txns.push(posting);
        let dues = card_due(&card(), &txns, &plans, TODAY, &WeekendCalendar::new()).unwrap();
        assert_eq!(dues[0].statement.charges, thb(1_450_050));
    }

    #[test]
    fn closed_plans_stop_posting() {
        let (txns, mut plans) = scenario();
        plans[0].closed_on = Some(date!(2026 - 10 - 31));
        let dues = card_due(&card(), &txns, &plans, TODAY, &WeekendCalendar::new()).unwrap();
        assert_eq!(dues[1].installments, thb(0));
        assert_eq!(dues[1].statement.charges, thb(100_000));
    }

    #[test]
    fn long_history_chains_balances() {
        // One purchase two years ago, never paid: interest compounds into each statement.
        let txns = [txn(
            TransactionKind::Expense,
            1_000_000,
            date!(2024 - 11 - 01),
        )];
        let rules = convert::card_rules(&card_settings(), Currency::Thb)
            .unwrap()
            .0;
        let entries: Vec<Entry> = txns
            .iter()
            .filter_map(|t| convert::card_entry(t, Currency::Thb).unwrap())
            .collect();
        let st = card_statements(
            &rules,
            Currency::Thb,
            &entries,
            |_| Ok(thb(0)),
            TODAY,
            &WeekendCalendar::new(),
        )
        .unwrap();
        assert_eq!(st.len(), 25);
        assert_eq!(st[0].period.statement_date, date!(2024 - 11 - 25));
        for w in st.windows(2) {
            assert_eq!(w[1].opening_balance, w[0].closing_balance);
            assert!(w[1].interest.minor() > 0);
        }
    }

    #[test]
    fn due_dates_skip_thai_bank_holidays() {
        // Cut-off 15 Nov 2026 + 20 days = Sat 5 Dec; Mon 7 Dec is a bank holiday.
        let mut c = card();
        c.rules.card.as_mut().unwrap().statement_day = 15;
        let accounts = vec![record(1, c)];
        let due = |cal: &WeekendCalendar| {
            this_month(&accounts, &[], &[], TODAY, cal)
                .items
                .iter()
                .map(|i| i.due_date)
                .max()
                .unwrap()
        };
        assert_eq!(due(&WeekendCalendar::new()), date!(2026 - 12 - 07));
        assert_eq!(due(&WeekendCalendar::thai([])), date!(2026 - 12 - 08));
        // A user's extra holiday on Tue 8 Dec moves it again.
        assert_eq!(
            due(&WeekendCalendar::thai([date!(2026 - 12 - 08)])),
            date!(2026 - 12 - 09)
        );
    }

    #[test]
    fn bad_settings_become_item_errors() {
        let mut broken = card();
        broken.name = "Broken".into();
        broken.rules.card.as_mut().unwrap().statement_day = 0;
        let mut no_settings = card();
        no_settings.name = "Bare".into();
        no_settings.rules.card = None;
        let accounts = vec![record(1, card()), record(2, broken), record(3, no_settings)];
        let m = this_month(&accounts, &[], &[], TODAY, &WeekendCalendar::new());
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.errors.len(), 2);
        assert_eq!(m.errors[0].account_name, "Broken");
        assert!(m.errors[0].message.starts_with("Statement cycle"));
        assert!(m.errors[1].message.contains("missing"));
    }

    fn loan() -> AccountData {
        AccountData {
            kind: AccountKind::Loan,
            name: "Loan".into(),
            currency: "THB".into(),
            rules: AccountRules {
                loan: Some(LoanSettings {
                    principal_minor: 1_200_000,
                    annual_rate_bp: 1_200,
                    months: 12,
                    start: date!(2026 - 09 - 05),
                    first_payment: date!(2026 - 10 - 05),
                    method: LoanMethod::Flat,
                    day_count: DayCount::Actual365,
                    rounding: RoundingSettings::default(),
                    installment_minor: None,
                }),
                ..AccountRules::default()
            },
            opened_on: None,
            closed_on: None,
        }
    }

    #[test]
    fn loan_next_payment() {
        // Flat 12% on ฿12,000 over 12 months: ฿1,000 principal + ฿120 interest a month.
        let d = loan_due(&loan(), &[], TODAY).unwrap().unwrap();
        assert_eq!(d.payment.date, date!(2026 - 11 - 05));
        assert_eq!(d.payment.amount, thb(112_000));
        assert_eq!((d.number, d.of), (2, 12));
        assert_eq!(d.status, Status::Overdue);

        let paid = vec![txn(
            TransactionKind::Payment,
            112_000,
            date!(2026 - 11 - 04),
        )];
        let d = loan_due(&loan(), &paid, TODAY).unwrap().unwrap();
        assert_eq!(d.status, Status::Paid);

        let d = loan_due(&loan(), &[], date!(2026 - 11 - 01))
            .unwrap()
            .unwrap();
        assert_eq!(d.status, Status::Unpaid);
        assert_eq!(loan_due(&loan(), &[], date!(2027 - 10 - 01)).unwrap(), None);
    }

    #[test]
    fn items_sorted_by_due_date() {
        let (txns, plans) = scenario();
        let accounts = vec![record(1, card()), record(2, loan())];
        let txns: Vec<_> = txns
            .into_iter()
            .enumerate()
            .map(|(i, mut t)| {
                t.account_id = Uuid::from_u128(1);
                record(100 + i as u128, t)
            })
            .collect();
        let plans: Vec<_> = plans
            .into_iter()
            .map(|mut p| {
                p.card_account_id = Uuid::from_u128(1);
                record(200, p)
            })
            .collect();
        let m = this_month(&accounts, &txns, &plans, TODAY, &WeekendCalendar::new());
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        let dates: Vec<Date> = m.items.iter().map(|i| i.due_date).collect();
        assert_eq!(
            dates,
            vec![
                date!(2026 - 11 - 05),
                date!(2026 - 11 - 16),
                date!(2026 - 12 - 15)
            ]
        );
    }
}
