//! Every user-facing string, in English and Thai.
//!
//! Plain strings live in one table: each entry has both languages, so a missing
//! translation doesn't compile. Strings with values, and labels for API enums, are the
//! functions further down; they `match` on both the value and the language.

use std::sync::atomic::{AtomicU8, Ordering};

use billo_api::rules::{DayCount, DueShift, InterestStart, LoanMethod};
use billo_api::{AccountKind, CategoryKind, TransactionKind};
use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "Billo";

/// The UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Th,
}

impl Lang {
    pub const ALL: [Self; 2] = [Self::Th, Self::En];

    /// Thai for any `th…` locale (`th`, `th-TH`, `th_TH.UTF-8`), otherwise English.
    pub fn from_locale(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("th") {
            Self::Th
        } else {
            Self::En
        }
    }

    /// The language's name in itself, for the switch.
    pub const fn own_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Th => "ไทย",
        }
    }

    /// `"en"` / `"th"`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Th => "th",
        }
    }
}

/// The language as a number: 0 English, 1 Thai. Global in the app; per thread in
/// tests, so tests that switch language don't affect each other.
#[cfg(not(test))]
static CURRENT: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    static CURRENT: AtomicU8 = const { AtomicU8::new(0) };
}

fn current() -> u8 {
    #[cfg(not(test))]
    return CURRENT.load(Ordering::Relaxed);
    #[cfg(test)]
    return CURRENT.with(|c| c.load(Ordering::Relaxed));
}

/// The language in use. English until [`set_lang`] is called.
pub fn lang() -> Lang {
    match current() {
        1 => Lang::Th,
        _ => Lang::En,
    }
}

pub fn set_lang(l: Lang) {
    let n = match l {
        Lang::En => 0,
        Lang::Th => 1,
    };
    #[cfg(not(test))]
    CURRENT.store(n, Ordering::Relaxed);
    #[cfg(test)]
    CURRENT.with(|c| c.store(n, Ordering::Relaxed));
}

/// The current language's half of an `(English, Thai)` pair.
fn pick<T>((en, th): (T, T)) -> T {
    match lang() {
        Lang::En => en,
        Lang::Th => th,
    }
}

macro_rules! table {
    ($($name:ident: $en:literal, $th:literal;)*) => {
        /// One plain string.
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Key { $($name,)* }

        impl Key {
            #[cfg(test)]
            pub const ALL: &[Self] = &[$(Self::$name,)*];
        }

        /// `(English, Thai)`. Exhaustive: every key has both.
        pub const fn pair(key: Key) -> (&'static str, &'static str) {
            match key { $(Key::$name => ($en, $th),)* }
        }

        $(
            pub fn $name() -> &'static str {
                pick(pair(Key::$name))
            }
        )*
    };
}

table! {
    sign_in: "Sign in", "เข้าสู่ระบบ";
    signing_in: "Signing in…", "กำลังเข้าสู่ระบบ…";
    sign_out: "Sign out", "ออกจากระบบ";
    signing_out: "Signing out…", "กำลังออกจากระบบ…";
    unsynced_title: "Changes not synced yet", "ยังมีการเปลี่ยนแปลงที่ยังไม่ได้ซิงค์";
    stay_signed_in: "Stay signed in", "อยู่ในระบบต่อ";
    sign_out_discard: "Sign out and discard", "ออกจากระบบและทิ้งการเปลี่ยนแปลง";
    email: "Email", "อีเมล";
    email_placeholder: "you@example.com", "you@example.com";
    dev_sign_in_note: "Development sign-in: enter any email. Google sign-in comes later.", "เข้าสู่ระบบสำหรับนักพัฒนา: ใส่อีเมลใดก็ได้ ส่วนการเข้าสู่ระบบด้วย Google จะตามมาภายหลัง";
    dev_login_off: "This server doesn't allow development sign-in (DEV_LOGIN is off). Google sign-in isn't available yet.", "เซิร์ฟเวอร์นี้ไม่เปิดให้เข้าสู่ระบบแบบนักพัฒนา (DEV_LOGIN ปิดอยู่) และยังใช้การเข้าสู่ระบบด้วย Google ไม่ได้";
    email_required: "Enter an email address.", "กรุณากรอกอีเมล";
    signed_in_as: "Signed in as", "เข้าสู่ระบบในชื่อ";
    server: "Server", "เซิร์ฟเวอร์";
    app_label: "Billo app", "แอป Billo";
    account_title: "Account", "บัญชีผู้ใช้";
    about: "About", "เกี่ยวกับ";
    holidays_title: "Bank holidays", "วันหยุดธนาคาร";
    holidays_note: "Due dates set to move off non-business days skip weekends and the Bank of Thailand's bank holidays for 2026 and 2027. Add any other day your bank is closed, such as a regional holiday.", "วันครบกำหนดชำระที่ตั้งให้เลื่อนเมื่อตรงกับวันหยุด จะข้ามวันเสาร์-อาทิตย์และวันหยุดสถาบันการเงินตามประกาศธนาคารแห่งประเทศไทยปี 2569 และ 2570 คุณเพิ่มวันอื่นที่ธนาคารปิดได้ เช่น วันหยุดเฉพาะพื้นที่";
    extra_holidays: "Your extra holidays", "วันหยุดที่คุณเพิ่ม";
    no_extra_holidays: "None added.", "ยังไม่ได้เพิ่ม";
    holidays_device_only: "Saved on this device only.", "บันทึกไว้ในอุปกรณ์นี้เท่านั้น";
    holiday_date: "Holiday date", "วันหยุด";
    remove: "Remove", "เอาออก";
    offline: "offline", "ออฟไลน์";
    offline_label: "Offline", "ออฟไลน์";
    sync_title: "Sync", "การซิงค์";
    sync_synced: "Synced", "ซิงค์แล้ว";
    sync_syncing: "Syncing…", "กำลังซิงค์…";
    sync_failed: "Couldn't sync", "ซิงค์ไม่สำเร็จ";
    sync_now: "Sync now", "ซิงค์ตอนนี้";
    conflict_notice: "Something you changed was also changed on another device. Your latest change was kept.", "รายการที่คุณแก้ไขถูกแก้ไขจากอุปกรณ์อื่นด้วย ระบบเก็บการแก้ไขล่าสุดของคุณไว้";
    local_store_failed: "Billo couldn't open its storage on this device, so changes are kept only until you close the app.", "Billo เปิดพื้นที่จัดเก็บบนอุปกรณ์นี้ไม่ได้ การเปลี่ยนแปลงจะอยู่จนกว่าคุณจะปิดแอป";
    dismiss: "OK", "ตกลง";
    retry: "Retry", "ลองอีกครั้ง";
    loading: "Loading…", "กำลังโหลด…";
    save: "Save", "บันทึก";
    edit: "Edit", "แก้ไข";
    delete: "Delete", "ลบ";
    confirm_delete: "Tap again to delete", "แตะอีกครั้งเพื่อยืนยันการลบ";
    back: "Back", "กลับ";
    yes: "Yes", "ใช่";
    no: "No", "ไม่";
    this_month: "This month", "เดือนนี้";
    accounts: "Accounts", "บัญชี";
    add_account: "Add account", "เพิ่มบัญชี";
    edit_account: "Edit account", "แก้ไขบัญชี";
    no_accounts: "No accounts yet. Add your cards, loans and bank accounts to get started.", "ยังไม่มีบัญชี เริ่มต้นด้วยการเพิ่มบัตร สินเชื่อ และบัญชีธนาคารของคุณ";
    name: "Name", "ชื่อ";
    currency: "Currency", "สกุลเงิน";
    example_values: "The values below are only examples. Change them to match your card's terms (see your statement or the bank's fee schedule).", "ค่าด้านล่างเป็นเพียงตัวอย่าง ปรับให้ตรงกับเงื่อนไขบัตรของคุณ (ดูได้จากใบแจ้งยอดหรือตารางค่าธรรมเนียมของธนาคาร)";
    statement_and_due: "Statement and due date", "วันสรุปยอดและวันครบกำหนดชำระ";
    statement_day: "Statement day", "วันสรุปยอด";
    statement_day_hint: "Day of the month the statement is cut (1–31; 31 means the last day of the month).", "วันที่ของเดือนที่ตัดรอบบัญชี (1–31 โดย 31 หมายถึงวันสิ้นเดือน)";
    due_rule: "Payment due", "กำหนดชำระ";
    due_mode_days: "Days after statement", "นับจากวันสรุปยอด";
    due_mode_day: "On a day of the month", "วันที่ประจำเดือน";
    days_after_statement: "Days after", "จำนวนวันหลังสรุปยอด";
    due_day_of_month: "Day of month", "วันที่ของเดือน";
    days_after_hint: "e.g. 20 means due 20 days after the statement date.", "เช่น 20 หมายถึงครบกำหนดชำระ 20 วันหลังวันสรุปยอด";
    due_day_hint: "The first time this day comes round after the statement date.", "ใช้วันที่นี้ครั้งแรกที่มาถึงหลังวันสรุปยอด";
    due_date: "Due date", "วันครบกำหนดชำระ";
    if_not_business_day: "If due on a weekend", "ถ้าครบกำหนดตรงวันหยุด";
    minimum_payment: "Minimum payment", "ยอดชำระขั้นต่ำ";
    min_percent: "Percent of balance (%)", "ร้อยละของยอดคงค้าง (%)";
    min_floor: "But at least", "แต่ไม่น้อยกว่า";
    min_hint: "Installment portions and any overdue minimum are always added in full.", "ยอดผ่อนรายเดือนและยอดขั้นต่ำที่ค้างจากรอบก่อนจะรวมเต็มจำนวนเสมอ";
    or_at_least: "or at least", "หรืออย่างน้อย";
    round_to: "Round to", "ปัดเศษทีละ";
    rounding: "Rounding", "การปัดเศษ";
    round_nearest: "Nearest", "ปัดใกล้สุด";
    round_up: "Up", "ปัดขึ้น";
    round_down: "Down", "ปัดลง";
    interest: "Interest", "ดอกเบี้ย";
    interest_rate: "Interest rate", "อัตราดอกเบี้ย";
    interest_rate_year: "Interest rate (% a year)", "อัตราดอกเบี้ย (% ต่อปี)";
    per_year: "a year", "ต่อปี";
    day_count: "Days in a year", "จำนวนวันใน 1 ปี";
    interest_from: "Interest counted from", "เริ่มคิดดอกเบี้ยจาก";
    grace_period: "Interest-free if paid in full", "ปลอดดอกเบี้ยเมื่อชำระเต็มจำนวน";
    grace_label: "No interest on new purchases when the last statement was paid in full", "ไม่คิดดอกเบี้ยรายการใช้จ่ายใหม่ ถ้าชำระยอดรอบก่อนเต็มจำนวน";
    credit_limit: "Credit limit", "วงเงิน";
    credit_limit_optional: "Credit limit (optional)", "วงเงิน (ไม่บังคับ)";
    loan_terms: "Loan terms", "เงื่อนไขสินเชื่อ";
    amount_borrowed: "Amount borrowed", "ยอดเงินกู้";
    months: "Months", "จำนวนงวด (เดือน)";
    months_short: "months", "เดือน";
    start_date: "Start date", "วันเริ่มสัญญา";
    first_payment: "First payment", "วันชำระงวดแรก";
    method: "Interest method", "วิธีคิดดอกเบี้ย";
    stated_installment: "Monthly installment", "ค่างวดรายเดือน";
    stated_installment_optional: "Monthly installment (optional)", "ค่างวดรายเดือน (ไม่บังคับ)";
    stated_installment_hint: "If your contract states the monthly amount, enter it; otherwise Billo works it out.", "ถ้าสัญญาระบุค่างวดไว้ ให้กรอกตามนั้น ถ้าไม่ระบุ Billo จะคำนวณให้";
    transactions: "Transactions", "รายการ";
    add: "Add", "เพิ่ม";
    no_transactions: "No transactions yet.", "ยังไม่มีรายการ";
    add_transaction: "Add transaction", "เพิ่มรายการ";
    edit_transaction: "Edit transaction", "แก้ไขรายการ";
    type_label: "Type", "ประเภท";
    amount: "Amount", "จำนวนเงิน";
    date: "Date", "วันที่ทำรายการ";
    posting_date: "Posting date", "วันที่รายการเข้าบัตร";
    posting_date_hint: "When the charge reached the card. Leave blank if it's the same day.", "วันที่ยอดเข้าบัตร เว้นว่างไว้ถ้าเป็นวันเดียวกัน";
    posted: "posted", "เข้าบัตร";
    category: "Category", "หมวดหมู่";
    no_category: "No category", "ไม่ระบุหมวดหมู่";
    new_category_option: "+ New category…", "+ หมวดหมู่ใหม่…";
    new_category_name: "New category name", "ชื่อหมวดหมู่ใหม่";
    note: "Note", "บันทึกช่วยจำ";
    interest_adjust_hint: "Interest the bank charged that Billo didn't work out. Use a minus sign for a reversal.", "ดอกเบี้ยที่ธนาคารเรียกเก็บแต่ Billo ไม่ได้คำนวณไว้ ใส่เครื่องหมายลบถ้าเป็นการคืนดอกเบี้ย";
    installment_plans: "Installment plans", "รายการผ่อนชำระ";
    no_plans: "No installment plans.", "ไม่มีรายการผ่อนชำระ";
    add_plan: "Add installment plan", "เพิ่มรายการผ่อน";
    edit_plan: "Edit installment plan", "แก้ไขรายการผ่อน";
    plan_amount: "Purchase amount", "ยอดซื้อ";
    monthly_rate: "Monthly rate (%)", "ดอกเบี้ยต่อเดือน (%)";
    monthly_rate_hint: "0 for 0% plans.", "ใส่ 0 สำหรับผ่อน 0%";
    first_posting: "First posting date", "วันที่เรียกเก็บงวดแรก";
    first_posting_hint: "When the first monthly portion appears on the card; the rest follow on the same day each month.", "วันที่ยอดผ่อนงวดแรกเข้าบัตร งวดถัดไปจะเรียกเก็บวันเดียวกันของทุกเดือน";
    first_posting_short: "from", "เริ่ม";
    per_month: "/month", "/เดือน";
    per_month_rate: "a month", "ต่อเดือน";
    invalid_plan: "Can't read this plan", "อ่านรายการผ่อนนี้ไม่ได้";
    today: "Today:", "วันนี้:";
    home_empty: "Nothing here yet. Add your credit cards and loans to see what's due this month.", "ยังไม่มีข้อมูล เพิ่มบัตรเครดิตและสินเชื่อเพื่อดูยอดที่ต้องชำระเดือนนี้";
    nothing_due: "Nothing due on your cards or loans.", "ไม่มียอดที่ต้องชำระสำหรับบัตรหรือสินเชื่อ";
    cant_work_out: "Can't work out what's due", "คำนวณยอดที่ต้องชำระไม่ได้";
    status_nothing_due: "Nothing due", "ไม่มียอดชำระ";
    status_paid_in_full: "Paid in full", "ชำระเต็มจำนวนแล้ว";
    status_paid: "Paid", "ชำระแล้ว";
    status_minimum_paid: "Minimum paid", "ชำระขั้นต่ำแล้ว";
    status_unpaid: "Unpaid", "ยังไม่ชำระ";
    status_overdue: "Overdue", "เกินกำหนดชำระ";
    status_open: "Not cut yet", "ยังไม่สรุปยอด";
    next_statement: "Next statement", "สรุปยอดครั้งถัดไป";
    statement_of: "Statement of", "ใบแจ้งยอดวันที่";
    opening_balance: "Opening balance", "ยอดยกมา";
    new_charges: "New charges", "รายการใช้จ่ายใหม่";
    payments_and_credits: "Payments and refunds", "ชำระเงินและเงินคืน";
    closing_balance: "Closing balance", "ยอดคงเหลือ";
    minimum_due: "Minimum due", "ยอดชำระขั้นต่ำ";
    balance_so_far: "Balance so far", "ยอดคงเหลือถึงตอนนี้";
    minimum_so_far: "Minimum so far", "ยอดขั้นต่ำถึงตอนนี้";
    paid_since_statement: "Paid since statement", "ชำระแล้วหลังสรุปยอด";
    overdue_from_last: "Overdue from last statement", "ยอดค้างชำระจากรอบก่อน";
    open_note: "This statement isn't cut yet; amounts may still change.", "รอบนี้ยังไม่สรุปยอด ตัวเลขอาจเปลี่ยนแปลงได้";
    payment: "Payment", "ค่างวด";
    principal: "Principal", "เงินต้น";
    balance_after: "Balance after", "ยอดคงเหลือหลังชำระ";
    paid: "Paid", "ชำระแล้ว";
    more: "More", "เพิ่มเติม";
    categories: "Categories", "หมวดหมู่";
    add_category: "Add category", "เพิ่มหมวดหมู่";
    edit_category: "Edit category", "แก้ไขหมวดหมู่";
    no_categories: "No categories yet.", "ยังไม่มีหมวดหมู่";
    name_th: "Thai name", "ชื่อภาษาไทย";
    name_en: "English name", "ชื่อภาษาอังกฤษ";
    category_name_hint: "Fill in one or both; Billo shows the one for your language.", "กรอกชื่อใดชื่อหนึ่งหรือทั้งสองชื่อ Billo จะแสดงตามภาษาที่คุณเลือก";
    category_name_required: "Enter a Thai or English name.", "กรุณากรอกชื่อภาษาไทยหรือภาษาอังกฤษ";
    spending: "Spending", "รายจ่าย";
    income: "Income", "รายรับ";
    parent: "Inside", "อยู่ในหมวด";
    no_parent: "Nothing (top level)", "ไม่มี (หมวดหลัก)";
    category_delete_note: "Transactions in this category keep their amounts; they just lose the category.", "รายการในหมวดนี้ยังอยู่ครบ เพียงแต่จะไม่มีหมวดหมู่";
    starter_title: "Start with common categories?", "เริ่มด้วยหมวดหมู่ยอดนิยมไหม?";
    starter_body: "Food, transport, bills, salary and other everyday categories, in Thai and English. You can rename or delete any of them.", "อาหาร การเดินทาง ค่าน้ำค่าไฟ เงินเดือน และหมวดอื่น ๆ ที่ใช้ในชีวิตประจำวัน ทั้งภาษาไทยและอังกฤษ แก้ชื่อหรือลบได้ทุกหมวด";
    add_starter: "Add starter categories", "เพิ่มหมวดหมู่เริ่มต้น";
    adding: "Adding…", "กำลังเพิ่ม…";
    not_now: "Not now", "ไว้ทีหลัง";
    budgets: "Budgets", "งบประมาณ";
    previous_month: "Previous month", "เดือนก่อน";
    next_month: "Next month", "เดือนถัดไป";
    budgeted: "Budgeted", "งบที่ตั้งไว้";
    spent: "Spent", "ใช้ไป";
    left: "Left", "คงเหลือ";
    over_budget: "Over budget", "เกินงบ";
    unbudgeted: "Spending without a budget", "รายจ่ายที่ไม่ได้ตั้งงบ";
    set_limit: "Set limit", "ตั้งงบ";
    monthly_limit: "Limit for this month", "งบของเดือนนี้";
    remove_limit: "Remove limit", "ลบงบ";
    cancel: "Cancel", "ยกเลิก";
    not_budgeted: "No limit yet", "ยังไม่ได้ตั้งงบ";
    budgets_need_categories: "Add spending categories first, then set a limit for each.", "เพิ่มหมวดหมู่รายจ่ายก่อน แล้วตั้งงบให้แต่ละหมวด";
    budgets_note: "Purchases and fees count as spending; refunds reduce it. Amounts in baht only.", "รายการซื้อและค่าธรรมเนียมนับเป็นรายจ่าย ส่วนเงินคืนจะนำมาหักออก นับเฉพาะยอดเงินบาท";
    cant_work_out_budget: "Can't work out this month's budget", "คำนวณงบเดือนนี้ไม่ได้";
    language: "Language", "ภาษา";
    err_network: "Can't reach the server", "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้";
    err_session_ended: "Your session has ended. Please sign in again.", "เซสชันหมดอายุแล้ว กรุณาเข้าสู่ระบบอีกครั้ง";
    err_not_found: "Not found", "ไม่พบข้อมูล";
    err_not_saved: "Not saved", "บันทึกไม่สำเร็จ";
    err_conflict: "Conflict", "ข้อมูลขัดแย้งกัน";
    err_server: "Server error", "เซิร์ฟเวอร์ขัดข้อง";
    err_decode: "Unexpected response from the server", "เซิร์ฟเวอร์ตอบกลับในรูปแบบที่ไม่รองรับ";
    required: "required", "จำเป็นต้องกรอก";
    not_a_number: "not a number", "ไม่ใช่ตัวเลข";
    whole_numbers_only: "whole numbers only", "ใส่ได้เฉพาะจำนวนเต็ม";
    cant_be_negative: "can't be negative", "ติดลบไม่ได้";
    too_large: "too large", "มากเกินไป";
    not_a_date: "not a valid date", "วันที่ไม่ถูกต้อง";
    more_than_zero: "must be more than zero", "ต้องมากกว่าศูนย์";
    not_before_date: "can't be before the date", "ต้องไม่ก่อนวันที่ทำรายการ";
    unknown: "unknown", "ไม่พบในระบบ";
    min_amount: "Minimum amount", "ยอดขั้นต่ำอย่างน้อย";
    min_rounding: "Minimum rounding", "การปัดเศษยอดขั้นต่ำ";
    interest_rounding: "Interest rounding", "การปัดเศษดอกเบี้ย";
}

pub fn kind_label(k: AccountKind) -> &'static str {
    pick(match k {
        AccountKind::Cash => ("Cash", "เงินสด"),
        AccountKind::Bank => ("Bank account", "บัญชีธนาคาร"),
        AccountKind::Card => ("Credit card", "บัตรเครดิต"),
        AccountKind::Revolving => ("Cash card / credit line", "บัตรกดเงินสด / วงเงินหมุนเวียน"),
        AccountKind::Loan => ("Personal loan", "สินเชื่อส่วนบุคคล"),
    })
}

pub fn kind_group(k: AccountKind) -> &'static str {
    pick(match k {
        AccountKind::Cash => ("Cash", "เงินสด"),
        AccountKind::Bank => ("Bank accounts", "บัญชีธนาคาร"),
        AccountKind::Card => ("Credit cards", "บัตรเครดิต"),
        AccountKind::Revolving => ("Cash cards", "บัตรกดเงินสด"),
        AccountKind::Loan => ("Loans", "สินเชื่อ"),
    })
}

pub fn kind_hint(k: AccountKind) -> &'static str {
    pick(match k {
        AccountKind::Cash => ("Money in your wallet", "เงินในกระเป๋า"),
        AccountKind::Bank => ("Savings or current account", "บัญชีออมทรัพย์หรือกระแสรายวัน"),
        AccountKind::Card => (
            "Monthly statements, minimum payments, installments",
            "ใบแจ้งยอดรายเดือน ยอดชำระขั้นต่ำ และการผ่อนชำระ",
        ),
        AccountKind::Revolving => (
            "A credit line that charges daily interest",
            "วงเงินที่คิดดอกเบี้ยเป็นรายวัน",
        ),
        AccountKind::Loan => (
            "Fixed monthly payments over a set term",
            "ผ่อนชำระเท่ากันทุกเดือนตามระยะเวลาที่กำหนด",
        ),
    })
}

pub fn new_account_title(k: AccountKind) -> &'static str {
    pick(match k {
        AccountKind::Cash => ("New cash account", "เพิ่มบัญชีเงินสด"),
        AccountKind::Bank => ("New bank account", "เพิ่มบัญชีธนาคาร"),
        AccountKind::Card => ("New credit card", "เพิ่มบัตรเครดิต"),
        AccountKind::Revolving => ("New cash card", "เพิ่มบัตรกดเงินสด"),
        AccountKind::Loan => ("New loan", "เพิ่มสินเชื่อ"),
    })
}

pub fn name_placeholder(k: AccountKind) -> &'static str {
    pick(match k {
        AccountKind::Cash => ("e.g. Wallet", "เช่น กระเป๋าสตางค์"),
        AccountKind::Bank => ("e.g. Savings", "เช่น บัญชีออมทรัพย์"),
        AccountKind::Card => ("e.g. Travel card", "เช่น บัตรเที่ยวต่างประเทศ"),
        AccountKind::Revolving => ("e.g. Cash card", "เช่น บัตรกดเงินสด"),
        AccountKind::Loan => ("e.g. Car repair loan", "เช่น สินเชื่อซ่อมรถ"),
    })
}

pub fn due_shift_label(d: DueShift) -> &'static str {
    pick(match d {
        DueShift::Next => ("Move to next business day", "เลื่อนไปวันทำการถัดไป"),
        DueShift::Previous => ("Move to previous business day", "เลื่อนมาวันทำการก่อนหน้า"),
        DueShift::None => ("Keep the date", "ไม่เลื่อน"),
    })
}

pub fn day_count_label(d: DayCount) -> &'static str {
    pick(match d {
        DayCount::Actual365 => ("365", "365"),
        DayCount::ActualActual => ("365 or 366 (leap years)", "365 หรือ 366 (ปีอธิกสุรทิน)"),
        DayCount::Actual360 => ("360", "360"),
    })
}

pub fn interest_start_label(i: InterestStart) -> &'static str {
    pick(match i {
        InterestStart::TransactionDate => ("Purchase date", "วันที่ใช้จ่าย"),
        InterestStart::PostingDate => ("Posting date", "วันที่รายการเข้าบัตร"),
    })
}

pub fn loan_method_label(m: LoanMethod) -> &'static str {
    pick(match m {
        LoanMethod::Effective => ("Effective rate (reducing balance)", "ดอกเบี้ยแบบลดต้นลดดอก"),
        LoanMethod::Flat => ("Flat rate", "ดอกเบี้ยคงที่ (Flat rate)"),
    })
}

pub fn transaction_kind_label(account: AccountKind, k: TransactionKind) -> &'static str {
    let card = matches!(account, AccountKind::Card | AccountKind::Revolving);
    pick(match k {
        TransactionKind::Expense if matches!(account, AccountKind::Revolving) => {
            ("Withdrawal", "เบิกเงินสด")
        }
        TransactionKind::Expense if card => ("Purchase", "ใช้จ่าย"),
        TransactionKind::Expense => ("Expense", "รายจ่าย"),
        TransactionKind::Income if card => ("Refund", "คืนเงิน"),
        TransactionKind::Income => ("Income", "รายรับ"),
        TransactionKind::Payment => ("Payment", "ชำระเงิน"),
        TransactionKind::Fee => ("Fee", "ค่าธรรมเนียม"),
        TransactionKind::Transfer => ("Transfer", "โอนเงิน"),
        TransactionKind::InterestAdjust => ("Interest charged", "ดอกเบี้ยที่ถูกเรียกเก็บ"),
    })
}

pub fn category_kind_label(k: CategoryKind) -> &'static str {
    match k {
        CategoryKind::Expense => spending(),
        CategoryKind::Income => income(),
    }
}

pub fn statement_on_day(day: u8) -> String {
    match lang() {
        Lang::En => format!("Statement on day {day}"),
        Lang::Th => format!("สรุปยอดทุกวันที่ {day}"),
    }
}

pub fn due_days_after(n: u16) -> String {
    match lang() {
        Lang::En => format!("due {n} days later"),
        Lang::Th => format!("ครบกำหนดชำระ {n} วันหลังสรุปยอด"),
    }
}

/// A version as shown to users: `v0.1.5 (12)`.
pub fn version_build(version: &str, build: u32) -> String {
    format!("v{version} ({build})")
}

pub fn due_on_day(day: u8) -> String {
    match lang() {
        Lang::En => format!("due on day {day}"),
        Lang::Th => format!("ครบกำหนดชำระวันที่ {day}"),
    }
}

pub fn includes_installments(amount: &str) -> String {
    match lang() {
        Lang::En => format!("Includes {amount} of installments, due in full."),
        Lang::Th => format!("รวมยอดผ่อนชำระ {amount} ซึ่งต้องชำระเต็มจำนวน"),
    }
}

pub fn payment_n_of(n: usize, of: usize) -> String {
    match lang() {
        Lang::En => format!("Payment {n} of {of}"),
        Lang::Th => format!("งวดที่ {n} จาก {of}"),
    }
}

pub fn spent_of(spent: &str, limit: &str) -> String {
    match lang() {
        Lang::En => format!("{spent} of {limit}"),
        Lang::Th => format!("{spent} จาก {limit}"),
    }
}

pub fn left_amount(amount: &str) -> String {
    match lang() {
        Lang::En => format!("{amount} left"),
        Lang::Th => format!("เหลือ {amount}"),
    }
}

pub fn changes_waiting(n: usize) -> String {
    match (lang(), n) {
        (Lang::En, 1) => "1 change waiting".to_owned(),
        (Lang::En, n) => format!("{n} changes waiting"),
        (Lang::Th, n) => format!("รอซิงค์ {n} รายการ"),
    }
}

/// Why sign-out stopped: `n` changes are still waiting.
pub fn unsynced_on_sign_out(n: usize, offline: bool) -> String {
    match (lang(), offline) {
        (Lang::En, offline) => {
            let what = if n == 1 {
                "1 change hasn't".to_owned()
            } else {
                format!("{n} changes haven't")
            };
            let why = if offline {
                "you're offline"
            } else {
                "the last sync failed"
            };
            format!(
                "{what} been sent to the server yet because {why}. If you sign out now, they will be lost. Stay signed in and Billo will send them when it can."
            )
        }
        (Lang::Th, offline) => {
            let why = if offline {
                "เพราะคุณออฟไลน์อยู่"
            } else {
                "เพราะการซิงค์ครั้งล่าสุดไม่สำเร็จ"
            };
            format!(
                "มีการเปลี่ยนแปลง {n} รายการที่ยังไม่ได้ส่งไปยังเซิร์ฟเวอร์{why} ถ้าออกจากระบบตอนนี้ การเปลี่ยนแปลงเหล่านี้จะหายไป หากอยู่ในระบบต่อ Billo จะส่งให้เมื่อทำได้"
            )
        }
    }
}

pub fn rejected_notice(n: u32) -> String {
    match (lang(), n) {
        (Lang::En, 1) => "1 change couldn't be saved on the server and was undone.".to_owned(),
        (Lang::En, n) => format!("{n} changes couldn't be saved on the server and were undone."),
        (Lang::Th, n) => format!("บันทึกการเปลี่ยนแปลง {n} รายการบนเซิร์ฟเวอร์ไม่ได้ จึงยกเลิกแล้ว"),
    }
}

pub fn over_by(amount: &str) -> String {
    match lang() {
        Lang::En => format!("{amount} over"),
        Lang::Th => format!("เกิน {amount}"),
    }
}

pub fn at_most_decimals(n: u32) -> String {
    match lang() {
        Lang::En => format!("at most {n} decimal places"),
        Lang::Th => format!("ทศนิยมได้ไม่เกิน {n} ตำแหน่ง"),
    }
}

pub fn between(min: i64, max: i64) -> String {
    match lang() {
        Lang::En => format!("must be {min} to {max}"),
        Lang::Th => format!("ต้องอยู่ระหว่าง {min} ถึง {max}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_thai(s: &str) -> bool {
        s.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
    }

    /// Strings that are the same in both languages on purpose.
    const SAME_IN_BOTH: &[Key] = &[Key::email_placeholder];

    #[test]
    fn every_key_has_english_and_thai() {
        for &k in Key::ALL {
            let (en, th) = pair(k);
            assert!(!en.trim().is_empty(), "{k:?}: English missing");
            assert!(!th.trim().is_empty(), "{k:?}: Thai missing");
            assert!(!has_thai(en), "{k:?}: Thai in the English text");
            if !SAME_IN_BOTH.contains(&k) {
                assert!(has_thai(th), "{k:?}: Thai text isn't Thai: {th}");
            }
        }
    }

    #[test]
    fn switching_language_switches_text() {
        set_lang(Lang::Th);
        assert_eq!(this_month(), "เดือนนี้");
        assert_eq!(kind_label(AccountKind::Card), "บัตรเครดิต");
        assert_eq!(payment_n_of(2, 12), "งวดที่ 2 จาก 12");
        set_lang(Lang::En);
        assert_eq!(this_month(), "This month");
        assert_eq!(payment_n_of(2, 12), "Payment 2 of 12");
    }

    #[test]
    fn unsynced_sign_out_text() {
        set_lang(Lang::En);
        assert!(unsynced_on_sign_out(1, true).starts_with("1 change hasn't"));
        assert!(unsynced_on_sign_out(3, false).contains("the last sync failed"));
        set_lang(Lang::Th);
        let th = unsynced_on_sign_out(3, true);
        assert!(th.contains("3 รายการ") && th.contains("ออฟไลน์"));
        set_lang(Lang::En);
    }

    #[test]
    fn locale_detection() {
        assert_eq!(Lang::from_locale("th-TH"), Lang::Th);
        assert_eq!(Lang::from_locale("TH"), Lang::Th);
        assert_eq!(Lang::from_locale("th_TH.UTF-8"), Lang::Th);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
    }

    #[test]
    fn language_codes_round_trip() {
        for l in Lang::ALL {
            let json = serde_json::to_string(&l).unwrap();
            assert_eq!(json, format!("\"{}\"", l.code()));
            assert_eq!(serde_json::from_str::<Lang>(&json).unwrap(), l);
        }
    }
}
