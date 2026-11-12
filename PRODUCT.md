# Billo — Product Brief

> Status: Draft v0.1 · 2026-11-12

## 1. One-liner

Billo is a personal finance app for Thai users who carry several credit cards, installment plans and loans. It records income and expenses and tells you three things: **what you owe this month**, **when you will be debt-free**, and **where your money goes**.

## 2. Problem

A typical Thai salaried worker may hold 3–5 credit cards, a few 0% installment plans (เผ่อน 0%), a cash card or card loan (บัตรกดเงินสด), and a personal loan (สินเชื่อส่วนบุคคล). Each one has its own statement cut-off date, due date, minimum payment and interest rule.

Today this is tracked in a spreadsheet or notes app. That breaks down because:

- Due dates are spread across the month, so payments get missed and late fees and interest pile up.
- Installments are hidden inside card statements, so the real monthly commitment is unclear.
- Revolving and effective-rate interest are hard to work out by hand, so the true cost and payoff date are unknown.
- Spending and debt live in different places, so there is no single picture.

## 3. Target user

**Primary (v1): the author.** Billo is built for my own use first. Each v1 feature must earn its place by replacing something I do in a spreadsheet today.

**Later:** Thai individuals in the same situation. The app is free and open source.

## 4. Goals and success criteria

v1 succeeds when:

1. **It replaces my spreadsheet.** I stop tracking debts and spending anywhere else.
2. **I never miss a due date.** No late fees from the time I adopt it.
3. **Its numbers match the bank.** Computed statement balances, interest, installment schedules and loan amortization match the real statements within **฿1**.

## 5. Scope: v1

### 5.1 Accounts and instruments

| Instrument | What Billo models |
|---|---|
| Cash / bank account | Balance, income and expense transactions, transfers |
| Credit card | Credit limit, statement cut-off day, due day, minimum-payment rule, revolving interest, statement generation |
| 0% / interest installment plan | Linked to a card. Principal, term, monthly amount, posting day, remaining months, early close |
| Cash card / card loan (revolving) | Credit line, daily-accrued interest, withdrawals, payments, minimum payment |
| Personal loan (term) | Principal, effective rate, term, schedule with principal/interest split, extra payments, early payoff |

### 5.2 Debt mechanics (must be accurate)

- **Card statement cycle:** cut-off, due date, statement balance, minimum payment, and interest when the balance is not paid in full.
- **Installments:** each monthly portion posts to the card's statement and counts toward minimum payment and available credit.
- **Loan amortization:** effective-rate (ลดต้นลดดอก) schedules, plus flat-rate where a product uses it. Extra and early payments recalculate the schedule.
- **Revolving credit / cash advance:** daily interest accrual on the outstanding balance.
- **Rules are configurable per product.** Rates, minimum-payment percentage and floor amount, grace periods and fees are data, not hard-coded. Defaults follow Bank of Thailand caps at time of writing (credit card ~16%/yr, personal loan / card loan ~25%/yr; minimum-payment % has changed in recent years). **These defaults must be checked against current BoT rules before release.**

### 5.3 Money tracking

- Income and expense transactions with categories (Thai-relevant defaults that the user can edit).
- **Multi-currency:** record foreign spending in its original currency with the THB amount charged. The THB amount is what counts toward card balances.
- **Monthly budgets** per category, showing actual vs. budget.

### 5.4 Views

- **This month:** every payment due (card minimums and full balances, installments, loan payments), sorted by due date, with paid/unpaid status.
- **Debt overview:** total debt, per-instrument balance, interest paid so far, projected payoff date, and total future interest.
- **Payoff planner:** compare snowball (smallest balance first) and avalanche (highest rate first) with a chosen extra monthly payment.
- **Cash flow:** income vs. expense by month and category, plus budget status.

### 5.5 Reminders

- In-app only: dashboard banners for payments due soon or overdue. No push notifications in v1.

### 5.6 Platform and data

- **Platforms:** Web, iOS, Android from a single codebase.
- **Auth:** Google sign-in.
- **Storage:** cloud-first. The server is the source of truth.
- **Offline:** reads from a local cache, queues writes while offline, and syncs on reconnect.
- **Language:** Thai and English UI. Supports Buddhist Era (พ.ศ.) and Gregorian date display.

## 6. Out of scope for v1

- Multi-user, shared, or household accounts.
- Bank connections, statement import (PDF/CSV), and slip/SMS parsing. Entry is manual only.
- Push or email notifications.
- Investments, assets and net worth (possible in v2).
- Tax features, bill payment, or moving money.

## 7. Technical direction

| Layer | Choice |
|---|---|
| Client UI | **Dioxus** (Rust), targeting web (WASM), iOS and Android |
| Shared domain | Rust crate containing all financial calculations (cycles, interest, amortization, payoff). Pure and deterministic, used by both client and server |
| Server | **Axum** (Rust) REST/JSON API |
| Database | **PostgreSQL** |
| Auth | Google OAuth / OIDC, verified by the server |
| Client cache | Local store (e.g. SQLite on mobile, IndexedDB on web) plus an outbound write queue |

**Engineering principles**

- **Money is never a float.** Use integer satang or a decimal type, and keep the currency code on every amount.
- **The domain crate is the product.** Golden tests built from real bank statements prove the ฿1 accuracy target.
- **Store events, derive balances.** Keep transactions and payments; compute statements and schedules from them, so changing a rule recalculates history correctly.
- **Privacy:** financial data is sensitive. Use TLS everywhere, encrypt at rest, collect no analytics by default, and support full data export (JSON/CSV) and account deletion.

## 8. Open questions

1. **Sync conflicts:** when two devices edit the same record offline, what should happen: last write wins, or a prompt to the user?
2. **Hosting:** where will the server run (self-hosted VPS or managed Postgres)? What happens to other users' data if it is open-sourced and others use the hosted instance?
3. **Thai bank presets:** should v1 ship presets for common cards and loans (KTC, KBank, SCB, UOB, Krungsri, etc.), or start with generic templates only?
4. **Rounding rules:** how does each bank round daily interest and installment amounts? This blocks the ฿1 target.
5. **FX handling:** record only the THB amount the bank charged, or also track FX rates and fees?
6. **License:** which open-source license (MIT/Apache-2.0 or AGPL)?
7. **Dioxus mobile maturity:** build a spike early to confirm iOS/Android builds, local storage and Google sign-in all work.

## 9. Milestones (proposed)

1. **Domain core:** a Rust crate with card cycles, installments, amortization and revolving interest, plus golden tests from my own statements.
2. **Server:** Axum + Postgres with Google auth and CRUD for accounts, instruments and transactions.
3. **Web client:** Dioxus web with the "This month" view, debt overview and transaction entry.
4. **Mobile:** Dioxus iOS/Android builds with offline cache and write queue.
5. **Planner and budgets:** snowball/avalanche planner, category budgets and cash-flow views.
6. **Dogfood:** run my real finances in Billo for 2 statement cycles and check the numbers against the bank.
