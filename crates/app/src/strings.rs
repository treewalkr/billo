//! Every user-facing string, in one place so a Thai translation can be added later.

pub const APP_NAME: &str = "Billo";

// Sign in
pub const SIGN_IN: &str = "Sign in";
pub const SIGNING_IN: &str = "Signing in…";
pub const SIGN_OUT: &str = "Sign out";
pub const EMAIL: &str = "Email";
pub const EMAIL_PLACEHOLDER: &str = "you@example.com";
pub const DEV_SIGN_IN_NOTE: &str =
    "Development sign-in: enter any email. Google sign-in comes later.";
pub const DEV_LOGIN_OFF: &str = "This server doesn't allow development sign-in (DEV_LOGIN is off). Google sign-in isn't available yet.";
pub const EMAIL_REQUIRED: &str = "Enter an email address.";
pub const SIGNED_IN_AS: &str = "Signed in as";

// Header
pub const SERVER: &str = "server";
pub const OFFLINE: &str = "offline";

// Generic
pub const RETRY: &str = "Retry";
pub const LOADING: &str = "Loading…";
pub const SAVE: &str = "Save";
pub const EDIT: &str = "Edit";
pub const DELETE: &str = "Delete";
pub const CONFIRM_DELETE: &str = "Tap again to delete";
pub const BACK: &str = "Back";
pub const YES: &str = "Yes";
pub const NO: &str = "No";

// Tabs
pub const THIS_MONTH: &str = "This month";
pub const ACCOUNTS: &str = "Accounts";

// Accounts
pub const ADD_ACCOUNT: &str = "Add account";
pub const EDIT_ACCOUNT: &str = "Edit account";
pub const NO_ACCOUNTS: &str =
    "No accounts yet. Add your cards, loans and bank accounts to get started.";
pub const NAME: &str = "Name";
pub const CURRENCY: &str = "Currency";
pub const EXAMPLE_VALUES: &str = "The values below are only examples. Change them to match your card's terms (see your statement or the bank's fee schedule).";
pub const STATEMENT_AND_DUE: &str = "Statement and due date";
pub const STATEMENT_DAY: &str = "Statement day";
pub const STATEMENT_DAY_HINT: &str =
    "Day of the month the statement is cut (1–31; 31 means the last day of the month).";
pub const DUE_RULE: &str = "Payment due";
pub const DUE_MODE_DAYS: &str = "Days after statement";
pub const DUE_MODE_DAY: &str = "On a day of the month";
pub const DAYS_AFTER_STATEMENT: &str = "Days after";
pub const DUE_DAY_OF_MONTH: &str = "Day of month";
pub const DAYS_AFTER_HINT: &str = "e.g. 20 means due 20 days after the statement date.";
pub const DUE_DAY_HINT: &str = "The first time this day comes round after the statement date.";
pub const DUE_DATE: &str = "Due date";
pub const IF_NOT_BUSINESS_DAY: &str = "If due on a weekend";
pub const MINIMUM_PAYMENT: &str = "Minimum payment";
pub const MIN_PERCENT: &str = "Percent of balance (%)";
pub const MIN_FLOOR: &str = "But at least";
pub const MIN_HINT: &str = "Installment portions and any overdue minimum are always added in full.";
pub const OR_AT_LEAST: &str = "or at least";
pub const ROUND_TO: &str = "Round to";
pub const ROUNDING: &str = "Rounding";
pub const ROUND_NEAREST: &str = "Nearest";
pub const ROUND_UP: &str = "Up";
pub const ROUND_DOWN: &str = "Down";
pub const INTEREST: &str = "Interest";
pub const INTEREST_RATE: &str = "Interest rate";
pub const INTEREST_RATE_YEAR: &str = "Interest rate (% a year)";
pub const PER_YEAR: &str = "a year";
pub const DAY_COUNT: &str = "Days in a year";
pub const INTEREST_FROM: &str = "Interest counted from";
pub const GRACE_PERIOD: &str = "Interest-free if paid in full";
pub const GRACE_LABEL: &str =
    "No interest on new purchases when the last statement was paid in full";
pub const CREDIT_LIMIT: &str = "Credit limit";
pub const CREDIT_LIMIT_OPTIONAL: &str = "Credit limit (optional)";
pub const LOAN_TERMS: &str = "Loan terms";
pub const AMOUNT_BORROWED: &str = "Amount borrowed";
pub const MONTHS: &str = "Months";
pub const MONTHS_SHORT: &str = "months";
pub const START_DATE: &str = "Start date";
pub const FIRST_PAYMENT: &str = "First payment";
pub const METHOD: &str = "Interest method";
pub const STATED_INSTALLMENT: &str = "Monthly installment";
pub const STATED_INSTALLMENT_OPTIONAL: &str = "Monthly installment (optional)";
pub const STATED_INSTALLMENT_HINT: &str =
    "If your contract states the monthly amount, enter it; otherwise Billo works it out.";

use billo_api::AccountKind;
use billo_api::rules::{DayCount, DueShift, InterestStart, LoanMethod};

pub const fn kind_label(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Cash => "Cash",
        AccountKind::Bank => "Bank account",
        AccountKind::Card => "Credit card",
        AccountKind::Revolving => "Cash card / credit line",
        AccountKind::Loan => "Personal loan",
    }
}

pub const fn kind_group(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Cash => "Cash",
        AccountKind::Bank => "Bank accounts",
        AccountKind::Card => "Credit cards",
        AccountKind::Revolving => "Cash cards",
        AccountKind::Loan => "Loans",
    }
}

pub const fn kind_hint(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Cash => "Money in your wallet",
        AccountKind::Bank => "Savings or current account",
        AccountKind::Card => "Monthly statements, minimum payments, installments",
        AccountKind::Revolving => "A credit line that charges daily interest",
        AccountKind::Loan => "Fixed monthly payments over a set term",
    }
}

pub const fn new_account_title(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Cash => "New cash account",
        AccountKind::Bank => "New bank account",
        AccountKind::Card => "New credit card",
        AccountKind::Revolving => "New cash card",
        AccountKind::Loan => "New loan",
    }
}

pub const fn name_placeholder(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Cash => "e.g. Wallet",
        AccountKind::Bank => "e.g. Savings",
        AccountKind::Card => "e.g. Travel card",
        AccountKind::Revolving => "e.g. Cash card",
        AccountKind::Loan => "e.g. Car repair loan",
    }
}

pub const fn due_shift_label(d: DueShift) -> &'static str {
    match d {
        DueShift::Next => "Move to next business day",
        DueShift::Previous => "Move to previous business day",
        DueShift::None => "Keep the date",
    }
}

pub const fn day_count_label(d: DayCount) -> &'static str {
    match d {
        DayCount::Actual365 => "365",
        DayCount::ActualActual => "365 or 366 (leap years)",
        DayCount::Actual360 => "360",
    }
}

pub const fn interest_start_label(i: InterestStart) -> &'static str {
    match i {
        InterestStart::TransactionDate => "Purchase date",
        InterestStart::PostingDate => "Posting date",
    }
}

pub const fn loan_method_label(m: LoanMethod) -> &'static str {
    match m {
        LoanMethod::Effective => "Effective rate (reducing balance)",
        LoanMethod::Flat => "Flat rate",
    }
}

pub fn statement_on_day(day: u8) -> String {
    format!("Statement on day {day}")
}

pub fn due_days_after(n: u16) -> String {
    format!("due {n} days later")
}

pub fn due_on_day(day: u8) -> String {
    format!("due on day {day}")
}

// Transactions
pub const TRANSACTIONS: &str = "Transactions";
pub const ADD: &str = "Add";
pub const NO_TRANSACTIONS: &str = "No transactions yet.";
pub const ADD_TRANSACTION: &str = "Add transaction";
pub const EDIT_TRANSACTION: &str = "Edit transaction";
pub const TYPE: &str = "Type";
pub const AMOUNT: &str = "Amount";
pub const DATE: &str = "Date";
pub const POSTING_DATE: &str = "Posting date";
pub const POSTING_DATE_HINT: &str =
    "When the charge reached the card. Leave blank if it's the same day.";
pub const POSTED: &str = "posted";
pub const CATEGORY: &str = "Category";
pub const NO_CATEGORY: &str = "No category";
pub const NEW_CATEGORY_OPTION: &str = "+ New category…";
pub const NEW_CATEGORY_NAME: &str = "New category name";
pub const NOTE: &str = "Note";
pub const INTEREST_ADJUST_HINT: &str =
    "Interest the bank charged that Billo didn't work out. Use a minus sign for a reversal.";

use billo_api::TransactionKind;

pub const fn transaction_kind_label(account: AccountKind, k: TransactionKind) -> &'static str {
    let card = matches!(account, AccountKind::Card | AccountKind::Revolving);
    match k {
        TransactionKind::Expense if matches!(account, AccountKind::Revolving) => "Withdrawal",
        TransactionKind::Expense if card => "Purchase",
        TransactionKind::Expense => "Expense",
        TransactionKind::Income if card => "Refund",
        TransactionKind::Income => "Income",
        TransactionKind::Payment => "Payment",
        TransactionKind::Fee => "Fee",
        TransactionKind::Transfer => "Transfer",
        TransactionKind::InterestAdjust => "Interest charged",
    }
}

// Installment plans
pub const INSTALLMENT_PLANS: &str = "Installment plans";
pub const NO_PLANS: &str = "No installment plans.";
pub const ADD_PLAN: &str = "Add installment plan";
pub const EDIT_PLAN: &str = "Edit installment plan";
pub const PLAN_AMOUNT: &str = "Purchase amount";
pub const MONTHLY_RATE: &str = "Monthly rate (%)";
pub const MONTHLY_RATE_HINT: &str = "0 for 0% plans.";
pub const FIRST_POSTING: &str = "First posting date";
pub const FIRST_POSTING_HINT: &str = "When the first monthly portion appears on the card; the rest follow on the same day each month.";
pub const FIRST_POSTING_SHORT: &str = "from";
pub const PER_MONTH: &str = "/month";
pub const PER_MONTH_RATE: &str = "a month";
pub const INVALID_PLAN: &str = "Can't read this plan";

// This month
pub const TODAY: &str = "Today:";
pub const HOME_EMPTY: &str =
    "Nothing here yet. Add your credit cards and loans to see what's due this month.";
pub const NOTHING_DUE: &str = "Nothing due on your cards or loans.";
pub const CANT_WORK_OUT: &str = "Can't work out what's due";
pub const STATUS_NOTHING_DUE: &str = "Nothing due";
pub const STATUS_PAID_IN_FULL: &str = "Paid in full";
pub const STATUS_PAID: &str = "Paid";
pub const STATUS_MINIMUM_PAID: &str = "Minimum paid";
pub const STATUS_UNPAID: &str = "Unpaid";
pub const STATUS_OVERDUE: &str = "Overdue";
pub const STATUS_OPEN: &str = "Not cut yet";
pub const NEXT_STATEMENT: &str = "Next statement";
pub const STATEMENT_OF: &str = "Statement of";
pub const OPENING_BALANCE: &str = "Opening balance";
pub const NEW_CHARGES: &str = "New charges";
pub const PAYMENTS_AND_CREDITS: &str = "Payments and refunds";
pub const CLOSING_BALANCE: &str = "Closing balance";
pub const MINIMUM_DUE: &str = "Minimum due";
pub const BALANCE_SO_FAR: &str = "Balance so far";
pub const MINIMUM_SO_FAR: &str = "Minimum so far";
pub const PAID_SINCE_STATEMENT: &str = "Paid since statement";
pub const OVERDUE_FROM_LAST: &str = "Overdue from last statement";
pub const OPEN_NOTE: &str = "This statement isn't cut yet; amounts may still change.";
pub const PAYMENT: &str = "Payment";
pub const PRINCIPAL: &str = "Principal";
pub const BALANCE_AFTER: &str = "Balance after";
pub const PAID: &str = "Paid";

pub fn includes_installments(amount: &str) -> String {
    format!("Includes {amount} of installments, due in full.")
}

pub fn payment_n_of(n: usize, of: usize) -> String {
    format!("Payment {n} of {of}")
}
