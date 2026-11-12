//! Term loans (personal loan, สินเชื่อส่วนบุคคล). Every number is a user setting.
//!
//! **Effective rate** (ลดต้นลดดอก): each payment pays the interest accrued on the
//! outstanding principal since the last payment (days `[previous, this)`), the rest
//! reduces principal. The last payment clears whatever is left. The installment is the
//! bank's stated amount, or the smallest amount (a multiple of the rounding step) that
//! clears the loan within the term.
//!
//! **Flat rate**: interest = principal × annual rate × years, spread evenly; one portion
//! absorbs the rounding remainder. Extra payments are not supported (early-payoff
//! discounts differ by contract).
//!
//! Extra payments (effective only) first pay accrued interest, then principal, and keep
//! the installment the same, so the term gets shorter.

use std::fmt;

use time::Date;

use crate::calendar::{add_months, clamped_date};
use crate::daycount::{DENOMINATOR, DayCount};
use crate::money::{Money, MoneyError};
use crate::rounding::{Rounding, RoundingMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanMethod {
    Effective(DayCount),
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanError {
    ZeroMonths,
    NonPositivePrincipal,
    /// Annual rate must be 0..=10_000 basis points.
    InvalidRate(u32),
    FirstPaymentNotAfterStart,
    /// The installment does not cover the interest, so the loan never shrinks.
    InstallmentTooSmall,
    NonPositiveExtraPayment,
    ExtraPaymentsNeedEffectiveRate,
    Money(MoneyError),
}

impl From<MoneyError> for LoanError {
    fn from(e: MoneyError) -> Self {
        Self::Money(e)
    }
}

impl fmt::Display for LoanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMonths => f.write_str("a loan needs at least 1 month"),
            Self::NonPositivePrincipal => f.write_str("loan principal must be positive"),
            Self::InvalidRate(bp) => {
                write!(f, "annual rate must be 0..=10000 basis points, got {bp}")
            }
            Self::FirstPaymentNotAfterStart => {
                f.write_str("first payment must be after the loan start")
            }
            Self::InstallmentTooSmall => f.write_str("installment does not cover the interest"),
            Self::NonPositiveExtraPayment => f.write_str("extra payment must be positive"),
            Self::ExtraPaymentsNeedEffectiveRate => {
                f.write_str("extra payments are only supported on effective-rate loans")
            }
            Self::Money(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LoanError {}

/// A loan's user settings, as entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoanSettings {
    pub principal: Money,
    /// 1_500 = 15% a year (flat or effective, per `method`).
    pub annual_rate_bp: u32,
    pub months: u16,
    pub start: Date,
    /// Later payments fall on the same day of following months (clamped to month end).
    pub first_payment: Date,
    pub method: LoanMethod,
    pub rounding: Rounding,
    /// The bank's stated monthly amount; `None` to compute it. Ignored for flat rate.
    pub installment: Option<Money>,
}

/// Validated loan settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoanTerms {
    principal: Money,
    annual_rate_bp: u32,
    months: u16,
    start: Date,
    first_payment: Date,
    method: LoanMethod,
    rounding: Rounding,
    installment: Option<Money>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentKind {
    Regular,
    Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoanPayment {
    pub kind: PaymentKind,
    pub date: Date,
    pub amount: Money,
    pub principal: Money,
    pub interest: Money,
    pub balance_after: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtraPayment {
    pub date: Date,
    pub amount: Money,
}

impl LoanTerms {
    pub fn new(settings: LoanSettings) -> Result<Self, LoanError> {
        let LoanSettings {
            principal,
            annual_rate_bp,
            months,
            start,
            first_payment,
            method,
            rounding,
            installment,
        } = settings;
        if months == 0 {
            return Err(LoanError::ZeroMonths);
        }
        if principal.minor() <= 0 {
            return Err(LoanError::NonPositivePrincipal);
        }
        if annual_rate_bp > 10_000 {
            return Err(LoanError::InvalidRate(annual_rate_bp));
        }
        if first_payment <= start {
            return Err(LoanError::FirstPaymentNotAfterStart);
        }
        if let Some(i) = installment {
            principal.checked_add(i)?;
        }
        Ok(Self {
            principal,
            annual_rate_bp,
            months,
            start,
            first_payment,
            method,
            rounding,
            installment,
        })
    }

    pub fn principal(&self) -> Money {
        self.principal
    }

    pub fn annual_rate_bp(&self) -> u32 {
        self.annual_rate_bp
    }

    pub fn months(&self) -> u16 {
        self.months
    }

    pub fn start(&self) -> Date {
        self.start
    }

    pub fn first_payment(&self) -> Date {
        self.first_payment
    }

    pub fn method(&self) -> LoanMethod {
        self.method
    }

    pub fn rounding(&self) -> Rounding {
        self.rounding
    }

    /// The `n`th regular payment date (1-based).
    pub fn payment_date(&self, n: u16) -> Date {
        let (y, m) = add_months(
            self.first_payment.year(),
            self.first_payment.month(),
            i32::from(n) - 1,
        );
        clamped_date(y, m, self.first_payment.day())
    }

    /// Monthly installment: the stated one, or computed.
    pub fn installment(&self) -> Result<Money, LoanError> {
        match (self.method, self.installment) {
            (LoanMethod::Flat, _) => Ok(self.flat_parts()?.0),
            (LoanMethod::Effective(_), Some(i)) => Ok(i),
            (LoanMethod::Effective(dc), None) => self.solve_installment(dc),
        }
    }

    /// Full repayment schedule, including any extra payments.
    pub fn schedule(&self, extras: &[ExtraPayment]) -> Result<Vec<LoanPayment>, LoanError> {
        match self.method {
            LoanMethod::Flat if !extras.is_empty() => {
                Err(LoanError::ExtraPaymentsNeedEffectiveRate)
            }
            LoanMethod::Flat => self.flat_schedule(),
            LoanMethod::Effective(dc) => {
                for e in extras {
                    self.principal.checked_add(e.amount)?;
                    if e.amount.minor() <= 0 {
                        return Err(LoanError::NonPositiveExtraPayment);
                    }
                }
                let mut extras = extras.to_vec();
                extras.sort_by_key(|e| e.date);
                self.effective_schedule(dc, self.installment()?, &extras)
            }
        }
    }

    /// Amount to close the loan on `date`: principal outstanding after payments made
    /// before `date`, plus interest accrued since the last of them.
    pub fn payoff_on(&self, date: Date, extras: &[ExtraPayment]) -> Result<Money, LoanError> {
        let schedule = self.schedule(extras)?;
        let paid: Vec<_> = schedule.iter().filter(|p| p.date < date).collect();
        let (balance, since) = paid
            .last()
            .map_or((self.principal, self.start), |p| (p.balance_after, p.date));
        let interest = match self.method {
            LoanMethod::Effective(dc) => self.interest_between(dc, balance.minor(), since, date)?,
            // Flat: the unpaid principal; any rebate on future interest is contract-specific.
            LoanMethod::Flat => 0,
        };
        let total = balance
            .minor()
            .checked_add(interest)
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(total, self.principal.currency()))
    }

    pub fn total_interest(&self, extras: &[ExtraPayment]) -> Result<Money, LoanError> {
        let minor = self
            .schedule(extras)?
            .iter()
            .map(|p| p.interest.minor())
            .sum();
        Ok(Money::new(minor, self.principal.currency()))
    }

    fn interest_between(
        &self,
        dc: DayCount,
        balance: i64,
        from: Date,
        to: Date,
    ) -> Result<i64, LoanError> {
        let n = dc.span_numerator(i128::from(balance), self.annual_rate_bp, from, to);
        Ok(self
            .rounding
            .apply(n, DENOMINATOR)
            .ok_or(MoneyError::Overflow)?)
    }

    fn effective_schedule(
        &self,
        dc: DayCount,
        installment: Money,
        extras: &[ExtraPayment],
    ) -> Result<Vec<LoanPayment>, LoanError> {
        let currency = self.principal.currency();
        let m = |minor| Money::new(minor, currency);
        let mut rows = Vec::new();
        let mut balance = self.principal.minor();
        let mut prev = self.start;
        let mut extras = extras.iter().peekable();

        for n in 1..=self.months {
            let due = self.payment_date(n);
            while let Some(e) = extras.next_if(|e| e.date < due) {
                let interest = self.interest_between(dc, balance, prev, e.date)?;
                let to_principal = (e.amount.minor() - interest).min(balance);
                let amount = interest + to_principal;
                balance -= to_principal;
                prev = e.date;
                rows.push(LoanPayment {
                    kind: PaymentKind::Extra,
                    date: e.date,
                    amount: m(amount),
                    principal: m(to_principal),
                    interest: m(interest),
                    balance_after: m(balance),
                });
                if balance == 0 {
                    return Ok(rows);
                }
            }
            let interest = self.interest_between(dc, balance, prev, due)?;
            let principal = if n == self.months || installment.minor() >= balance + interest {
                balance
            } else {
                installment.minor() - interest
            };
            if principal <= 0 && balance > 0 {
                return Err(LoanError::InstallmentTooSmall);
            }
            balance -= principal;
            prev = due;
            rows.push(LoanPayment {
                kind: PaymentKind::Regular,
                date: due,
                amount: m(principal + interest),
                principal: m(principal),
                interest: m(interest),
                balance_after: m(balance),
            });
            if balance == 0 {
                break;
            }
        }
        Ok(rows)
    }

    /// Smallest multiple of the rounding step whose schedule ends with a final payment
    /// no larger than the installment.
    fn solve_installment(&self, dc: DayCount) -> Result<Money, LoanError> {
        let currency = self.principal.currency();
        let step = self.rounding.step();
        let fits = |steps: i64| -> Result<bool, LoanError> {
            let inst = Money::new(steps * step, currency);
            match self.effective_schedule(dc, inst, &[]) {
                Ok(rows) => Ok(rows
                    .last()
                    .is_some_and(|r| r.amount.minor() <= inst.minor())),
                Err(LoanError::InstallmentTooSmall) => Ok(false),
                Err(e) => Err(e),
            }
        };
        // Upper bound: the whole principal plus all interest at the start balance.
        let mut hi = {
            let last = self.payment_date(self.months);
            let all = self.interest_between(dc, self.principal.minor(), self.start, last)?;
            (self.principal.minor() + all) / step + 1
        };
        let mut lo = 1;
        if !fits(hi)? {
            return Err(LoanError::InstallmentTooSmall);
        }
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if fits(mid)? { hi = mid } else { lo = mid + 1 }
        }
        Ok(Money::new(lo * step, currency))
    }

    /// (installment, per-row principal, per-row interest) for a flat-rate loan.
    fn flat_parts(&self) -> Result<(Money, Vec<i64>, Vec<i64>), LoanError> {
        let n = i64::from(self.months);
        let interest_total = self
            .rounding
            .apply(
                i128::from(self.principal.minor())
                    * i128::from(self.annual_rate_bp)
                    * i128::from(n),
                10_000 * 12,
            )
            .ok_or(MoneyError::Overflow)?;
        let total = self.principal.minor() + interest_total;
        let down = Rounding::new(self.rounding.step(), RoundingMode::Down).expect("step ≥ 1");
        let per = |x: i64| {
            down.apply(i128::from(x), i128::from(n))
                .ok_or(MoneyError::Overflow)
        };
        let (inst, p_each, i_each) = (
            per(total)?,
            per(self.principal.minor())?,
            per(interest_total)?,
        );
        let mut principal = vec![p_each; self.months.into()];
        let mut interest = vec![i_each; self.months.into()];
        let last = usize::from(self.months) - 1;
        principal[last] = self.principal.minor() - p_each * (n - 1);
        interest[last] = interest_total - i_each * (n - 1);
        // Keep each regular row equal to the installment by moving rounding into interest.
        for k in 0..last {
            interest[k] = inst - principal[k];
        }
        interest[last] = interest_total - interest[..last].iter().sum::<i64>();
        Ok((
            Money::new(inst, self.principal.currency()),
            principal,
            interest,
        ))
    }

    fn flat_schedule(&self) -> Result<Vec<LoanPayment>, LoanError> {
        let currency = self.principal.currency();
        let (_, principal, interest) = self.flat_parts()?;
        let mut balance = self.principal.minor();
        Ok((0..self.months)
            .map(|k| {
                let i = usize::from(k);
                balance -= principal[i];
                LoanPayment {
                    kind: PaymentKind::Regular,
                    date: self.payment_date(k + 1),
                    amount: Money::new(principal[i] + interest[i], currency),
                    principal: Money::new(principal[i], currency),
                    interest: Money::new(interest[i], currency),
                    balance_after: Money::new(balance, currency),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use time::macros::date;

    const fn satang(minor: i64) -> Money {
        Money::new(minor, Currency::Thb)
    }

    const fn thb(baht: i64) -> Money {
        satang(baht * 100)
    }

    /// ฿100,000 at 12% effective, 12 months, Jan 1 → first payment Feb 1 2026.
    fn effective(installment: Option<Money>, rounding: Rounding) -> LoanTerms {
        LoanTerms::new(LoanSettings {
            principal: thb(100_000),
            annual_rate_bp: 1_200,
            months: 12,
            start: date!(2026 - 01 - 01),
            first_payment: date!(2026 - 02 - 01),
            method: LoanMethod::Effective(DayCount::Actual365),
            rounding,
            installment,
        })
        .unwrap()
    }

    fn sum_principal(rows: &[LoanPayment]) -> i64 {
        rows.iter().map(|r| r.principal.minor()).sum()
    }

    #[test]
    fn effective_first_payment_and_full_repayment() {
        let loan = effective(Some(thb(8_885)), Rounding::default());
        let rows = loan.schedule(&[]).unwrap();
        // ฿100,000 × 12% × 31/365 = ฿1,019.178… → ฿1,019.18
        assert_eq!(rows[0].interest, satang(101_918));
        assert_eq!(rows[0].principal, satang(786_582));
        assert_eq!(rows.len(), 12);
        assert_eq!(sum_principal(&rows), thb(100_000).minor());
        assert_eq!(rows.last().unwrap().balance_after, thb(0));
        assert!(rows[..11].iter().all(|r| r.amount == thb(8_885)));
    }

    #[test]
    fn computed_installment_is_the_smallest_that_fits() {
        let loan = effective(None, Rounding::default());
        let inst = loan.installment().unwrap();
        // Textbook annuity is ~฿8,884.88; daily interest lands close to it.
        assert!((885_000..890_000).contains(&inst.minor()), "{inst}");
        let fits = |i: Money| {
            let rows = effective(Some(i), Rounding::default())
                .schedule(&[])
                .unwrap();
            rows.len() == 12 && rows.last().unwrap().amount.minor() <= i.minor()
        };
        assert!(fits(inst));
        assert!(!fits(satang(inst.minor() - 1)));
    }

    #[test]
    fn computed_installment_respects_rounding_step() {
        let loan = effective(None, Rounding::new(100, RoundingMode::Up).unwrap());
        assert_eq!(loan.installment().unwrap().minor() % 100, 0);
    }

    #[test]
    fn extra_payment_shortens_term_and_saves_interest() {
        let loan = effective(Some(thb(8_885)), Rounding::default());
        let base = loan.total_interest(&[]).unwrap();
        let extra = [ExtraPayment {
            date: date!(2026 - 04 - 15),
            amount: thb(30_000),
        }];
        let rows = loan.schedule(&extra).unwrap();
        assert!(
            rows.iter()
                .filter(|r| r.kind == PaymentKind::Regular)
                .count()
                < 12
        );
        assert_eq!(sum_principal(&rows), thb(100_000).minor());
        assert!(loan.total_interest(&extra).unwrap().minor() < base.minor());
        let x = rows.iter().find(|r| r.kind == PaymentKind::Extra).unwrap();
        assert_eq!(x.amount, thb(30_000));
        assert_eq!(
            x.principal.minor() + x.interest.minor(),
            thb(30_000).minor()
        );
    }

    #[test]
    fn extra_payment_larger_than_balance_closes_the_loan() {
        let loan = effective(Some(thb(8_885)), Rounding::default());
        let extra = [ExtraPayment {
            date: date!(2026 - 03 - 10),
            amount: thb(200_000),
        }];
        let rows = loan.schedule(&extra).unwrap();
        let last = rows.last().unwrap();
        assert_eq!(last.kind, PaymentKind::Extra);
        assert_eq!(last.balance_after, thb(0));
        assert!(last.amount.minor() < thb(200_000).minor());
    }

    #[test]
    fn payoff_amount_includes_accrued_interest() {
        let loan = effective(Some(thb(8_885)), Rounding::default());
        let rows = loan.schedule(&[]).unwrap();
        // After the Feb 1 payment, 10 days to Feb 11.
        let bal = rows[0].balance_after.minor();
        let expected = bal
            + Rounding::default()
                .apply(i128::from(bal) * 1_200 * 10, 10_000 * 365)
                .unwrap();
        assert_eq!(
            loan.payoff_on(date!(2026 - 02 - 11), &[]).unwrap(),
            satang(expected)
        );
        // Before any payment: principal + interest since start.
        assert_eq!(
            loan.payoff_on(date!(2026 - 01 - 01), &[]).unwrap(),
            thb(100_000)
        );
    }

    #[test]
    fn flat_rate_loan() {
        // ฿100,000 at 5% flat for 24 months: interest ฿10,000, ฿4,583.33 a month,
        // last payment ฿4,583.41.
        let loan = LoanTerms::new(LoanSettings {
            principal: thb(100_000),
            annual_rate_bp: 500,
            months: 24,
            start: date!(2026 - 01 - 01),
            first_payment: date!(2026 - 02 - 01),
            method: LoanMethod::Flat,
            rounding: Rounding::default(),
            installment: None,
        })
        .unwrap();
        assert_eq!(loan.installment().unwrap(), satang(458_333));
        let rows = loan.schedule(&[]).unwrap();
        assert!(rows[..23].iter().all(|r| r.amount == satang(458_333)));
        assert_eq!(rows[23].amount, satang(458_341));
        assert_eq!(sum_principal(&rows), thb(100_000).minor());
        assert_eq!(loan.total_interest(&[]).unwrap(), thb(10_000));
        assert_eq!(rows[23].balance_after, thb(0));
    }

    #[test]
    fn payment_dates_clamp_to_month_end() {
        let loan = LoanTerms::new(LoanSettings {
            principal: thb(10_000),
            annual_rate_bp: 1_200,
            months: 3,
            start: date!(2026 - 01 - 01),
            first_payment: date!(2026 - 01 - 31),
            method: LoanMethod::Effective(DayCount::Actual365),
            rounding: Rounding::default(),
            installment: None,
        })
        .unwrap();
        assert_eq!(loan.payment_date(2), date!(2026 - 02 - 28));
        assert_eq!(loan.payment_date(3), date!(2026 - 03 - 31));
    }

    #[test]
    fn rejects_bad_settings() {
        let d1 = date!(2026 - 01 - 01);
        let d2 = date!(2026 - 02 - 01);
        let m = LoanMethod::Effective(DayCount::Actual365);
        let r = Rounding::default();
        assert_eq!(
            LoanTerms::new(LoanSettings {
                principal: thb(1),
                annual_rate_bp: 0,
                months: 0,
                start: d1,
                first_payment: d2,
                method: m,
                rounding: r,
                installment: None
            }),
            Err(LoanError::ZeroMonths)
        );
        assert_eq!(
            LoanTerms::new(LoanSettings {
                principal: thb(0),
                annual_rate_bp: 0,
                months: 1,
                start: d1,
                first_payment: d2,
                method: m,
                rounding: r,
                installment: None
            }),
            Err(LoanError::NonPositivePrincipal)
        );
        assert_eq!(
            LoanTerms::new(LoanSettings {
                principal: thb(1),
                annual_rate_bp: 10_001,
                months: 1,
                start: d1,
                first_payment: d2,
                method: m,
                rounding: r,
                installment: None
            }),
            Err(LoanError::InvalidRate(10_001))
        );
        assert_eq!(
            LoanTerms::new(LoanSettings {
                principal: thb(1),
                annual_rate_bp: 0,
                months: 1,
                start: d2,
                first_payment: d1,
                method: m,
                rounding: r,
                installment: None
            }),
            Err(LoanError::FirstPaymentNotAfterStart)
        );
        let tiny = effective(Some(thb(100)), r);
        assert_eq!(tiny.schedule(&[]), Err(LoanError::InstallmentTooSmall));
        let flat = LoanTerms::new(LoanSettings {
            principal: thb(1_000),
            annual_rate_bp: 500,
            months: 2,
            start: d1,
            first_payment: d2,
            method: LoanMethod::Flat,
            rounding: r,
            installment: None,
        })
        .unwrap();
        assert_eq!(
            flat.schedule(&[ExtraPayment {
                date: d2,
                amount: thb(1)
            }]),
            Err(LoanError::ExtraPaymentsNeedEffectiveRate)
        );
    }
}
