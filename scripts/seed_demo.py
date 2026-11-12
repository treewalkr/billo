#!/usr/bin/env python3
"""Fill one Billo account with realistic Thai demo data (Sept–Nov 2026).

    python3 scripts/seed_demo.py                       # dev-login as demo@billo.dev
    python3 scripts/seed_demo.py --email someone@x     # dev-login as that email
    python3 scripts/seed_demo.py --token <bearer>      # an existing session's user
    python3 scripts/seed_demo.py --api http://localhost:8080

Safe to re-run: ids are derived from the user id, so a second run updates the same
records instead of adding new ones. Python 3 standard library only.
"""

import argparse
import datetime as dt
import json
import sys
import urllib.error
import urllib.request
import uuid

NAMESPACE = uuid.UUID("6f0e3b0c-6a43-4d3e-9a51-0b7f2d1c9e10")
THB = "THB"

# --- HTTP -------------------------------------------------------------------


class Api:
    def __init__(self, base, token=None):
        self.base = base.rstrip("/") + "/api/v1"
        self.token = token
        self.conflicts = 0

    def call(self, method, path, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, method=method)
        req.add_header("content-type", "application/json")
        if self.token:
            req.add_header("authorization", "Bearer " + self.token)
        try:
            with urllib.request.urlopen(req) as res:
                if res.headers.get("x-billo-conflict") == "1":
                    self.conflicts += 1
                raw = res.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as e:
            sys.exit(f"{method} {path} failed: {e.code} {e.read().decode(errors='replace')}")
        except urllib.error.URLError as e:
            sys.exit(f"can't reach {self.base}: {e.reason}")


# --- Helpers ------------------------------------------------------------------


def d(s):
    return dt.date.fromisoformat(s)


def baht(amount):
    """Baht (int, float or "1,234.50") to satang, without float surprises."""
    s = str(amount).replace(",", "")
    whole, _, frac = s.partition(".")
    return int(whole) * 100 + int((frac + "00")[:2])


def money(amount, currency=THB):
    return {"minor": baht(amount), "currency": currency}


def posting(day):
    """Card posting date: the next business day (weekends skipped)."""
    p = day + dt.timedelta(days=1)
    while p.weekday() >= 5:
        p += dt.timedelta(days=1)
    return p


def rounding(step=1):
    return {"step": step, "mode": "half_up"}


def card_rules(statement_day, due, shift, limit, min_bp, min_floor, rate_bp, grace):
    return {
        "v": 1,
        "card": {
            "statement_day": statement_day,
            "due": due,
            "due_shift": shift,
            "credit_limit_minor": baht(limit),
            "min_payment": {"percent_bp": min_bp, "floor_minor": baht(min_floor), "rounding": rounding()},
            "interest": {
                "annual_rate_bp": rate_bp,
                "day_count": "actual365",
                "start": "transaction_date",
                "rounding": rounding(),
                "grace_if_paid_in_full": grace,
            },
        },
    }


def loan_rules(principal, rate_bp, months, start, first, method, installment=None):
    loan = {
        "principal_minor": baht(principal),
        "annual_rate_bp": rate_bp,
        "months": months,
        "start": start,
        "first_payment": first,
        "method": method,
        "day_count": "actual365",
        "rounding": rounding(),
    }
    if installment is not None:
        loan["installment_minor"] = baht(installment)
    return {"v": 1, "loan": loan}


# --- The data -----------------------------------------------------------------

# The app's starter categories (crates/app/src/categories.rs), parents first.
STARTER = [
    ("expense", "อาหารและเครื่องดื่ม", "Food & drink", [("กาแฟและของว่าง", "Coffee & snacks")]),
    ("expense", "ของใช้เข้าบ้าน", "Groceries", []),
    (
        "expense",
        "การเดินทาง",
        "Transport",
        [("รถไฟฟ้า BTS/MRT", "BTS & MRT"), ("แท็กซี่และ Grab", "Taxi & Grab"), ("น้ำมันรถ", "Fuel")],
    ),
    ("expense", "ค่าเช่าที่พัก", "Rent & housing", []),
    ("expense", "ค่าน้ำค่าไฟ", "Bills & utilities", []),
    ("expense", "ค่าโทรศัพท์และอินเทอร์เน็ต", "Phone & internet", []),
    ("expense", "ช้อปปิ้ง", "Shopping", []),
    ("expense", "สุขภาพ", "Health", []),
    ("expense", "ประกัน", "Insurance", []),
    ("expense", "การศึกษา", "Education", []),
    ("expense", "บันเทิงและท่องเที่ยว", "Entertainment & travel", []),
    ("expense", "ส่งเงินให้ครอบครัว", "Family support", []),
    ("expense", "ทำบุญและบริจาค", "Merit & donations", []),
    ("income", "เงินเดือน", "Salary", []),
    ("income", "โบนัส", "Bonus", []),
    ("income", "รายได้อื่น", "Other income", []),
]

ACCOUNTS = {
    "bank": {"kind": "bank", "name": "KBank Savings", "opened_on": "2019-04-01", "rules": {"v": 1}},
    "cash": {"kind": "cash", "name": "Wallet", "opened_on": None, "rules": {"v": 1}},
    "everyday": {
        "kind": "card",
        "name": "Everyday card",
        "opened_on": "2022-03-15",
        "rules": card_rules(25, {"days_after_statement": 20}, "next", 80_000, 800, 1_000, 1600, True),
    },
    "travel": {
        "kind": "card",
        "name": "Travel card",
        "opened_on": "2024-07-01",
        "rules": card_rules(5, {"day_of_month": 25}, "next", 150_000, 800, 1_000, 1600, True),
    },
    "cashcard": {
        "kind": "revolving",
        "name": "Cash card",
        "opened_on": "2025-02-10",
        "rules": card_rules(16, {"day_of_month": 5}, "next", 50_000, 300, 500, 2500, False),
    },
    "ploan": {
        "kind": "loan",
        "name": "Personal loan",
        "opened_on": "2026-06-01",
        "rules": loan_rules(100_000, 1800, 24, "2026-06-01", "2026-07-05", "effective", installment=5_000),
    },
    "moto": {
        "kind": "loan",
        "name": "Motorbike loan (Honda Click)",
        "opened_on": "2025-12-15",
        "rules": loan_rules(45_000, 500, 36, "2025-12-15", "2026-01-15", "flat"),
    },
}

PLANS = {
    "phone": {
        "card": "everyday",
        "principal": "24,990",
        "months": 10,
        "monthly_rate_bp": 0,
        "first_post_on": "2026-09-25",
    },
    "aircon": {
        "card": "everyday",
        "principal": "18,900",
        "months": 6,
        "monthly_rate_bp": 69,
        "first_post_on": "2026-10-25",
    },
}

# (date, account, kind, category (English name) or None, amount in baht, note)
SPENDING = [
    # --- KBank Savings ---
    ("2026-09-01", "bank", "expense", "Rent & housing", "9,500", "Condo rent – Lumpini Ville"),
    ("2026-10-01", "bank", "expense", "Rent & housing", "9,500", "Condo rent – Lumpini Ville"),
    ("2026-11-01", "bank", "expense", "Rent & housing", "9,500", "Condo rent – Lumpini Ville"),
    ("2026-09-25", "bank", "income", "Salary", "45,000", "Salary"),
    ("2026-10-23", "bank", "income", "Salary", "45,000", "Salary"),
    ("2026-10-12", "bank", "income", "Other income", "3,500", "Freelance logo design"),
    ("2026-09-16", "bank", "expense", "Bills & utilities", "1,284.62", "MEA electricity"),
    ("2026-10-15", "bank", "expense", "Bills & utilities", "1,512.37", "MEA electricity"),
    ("2026-11-10", "bank", "expense", "Bills & utilities", "1,698.45", "MEA electricity"),
    ("2026-09-18", "bank", "expense", "Bills & utilities", "168.00", "MWA water"),
    ("2026-10-19", "bank", "expense", "Bills & utilities", "182.25", "MWA water"),
    ("2026-11-11", "bank", "expense", "Bills & utilities", "176.50", "MWA water"),
    ("2026-09-26", "bank", "expense", "Family support", "5,000", "To Mom (PromptPay)"),
    ("2026-10-24", "bank", "expense", "Family support", "5,000", "To Mom (PromptPay)"),
    ("2026-09-02", "bank", "expense", "BTS & MRT", "500", "Rabbit card top-up"),
    ("2026-09-17", "bank", "expense", "BTS & MRT", "500", "Rabbit card top-up"),
    ("2026-10-01", "bank", "expense", "BTS & MRT", "500", "Rabbit card top-up"),
    ("2026-10-16", "bank", "expense", "BTS & MRT", "500", "Rabbit card top-up"),
    ("2026-11-02", "bank", "expense", "BTS & MRT", "500", "Rabbit card top-up"),
    ("2026-10-08", "bank", "expense", "Insurance", "2,150", "AIA health premium (quarterly)"),
    ("2026-10-24", "bank", "fee", None, "15", "ATM fee (other bank)"),
    # --- Wallet ---
    ("2026-09-02", "cash", "expense", "Food & drink", "60", "Khao man gai"),
    ("2026-09-04", "cash", "expense", "Coffee & snacks", "65", "Café Amazon"),
    ("2026-09-08", "cash", "expense", "Food & drink", "75", "Pad kra pao"),
    ("2026-09-11", "cash", "expense", "Groceries", "112", "7-Eleven"),
    ("2026-09-15", "cash", "expense", "Fuel", "150", "PTT"),
    ("2026-09-22", "cash", "expense", "Food & drink", "80", "Boat noodles"),
    ("2026-09-29", "cash", "expense", "Taxi & Grab", "40", "Motorbike taxi"),
    ("2026-10-04", "cash", "expense", "Health", "12,500", "Dad's dental surgery"),
    ("2026-10-06", "cash", "expense", "Food & drink", "70", "Som tam & sticky rice"),
    ("2026-10-09", "cash", "expense", "Coffee & snacks", "75", "Café Amazon"),
    ("2026-10-13", "cash", "expense", "Fuel", "160", "PTT"),
    ("2026-10-17", "cash", "expense", "Merit & donations", "300", "Wat Pho donation"),
    ("2026-10-20", "cash", "expense", "Groceries", "245", "Or Tor Kor market"),
    ("2026-10-27", "cash", "expense", "Food & drink", "85", "Khao soi"),
    ("2026-11-02", "cash", "expense", "Coffee & snacks", "65", "Café Amazon"),
    ("2026-11-03", "cash", "expense", "Groceries", "89", "7-Eleven"),
    ("2026-11-04", "cash", "expense", "Food & drink", "60", "Khao man gai"),
    ("2026-11-05", "cash", "expense", "Taxi & Grab", "40", "Motorbike taxi"),
    ("2026-11-06", "cash", "expense", "Coffee & snacks", "75", "Café Amazon"),
    ("2026-11-06", "cash", "expense", "Food & drink", "75", "Pad kra pao"),
    ("2026-11-10", "cash", "expense", "Food & drink", "80", "Boat noodles"),
    ("2026-11-07", "cash", "expense", "Groceries", "132", "7-Eleven"),
    ("2026-11-09", "cash", "expense", "Coffee & snacks", "70", "Thai iced tea"),
    ("2026-11-09", "cash", "expense", "Groceries", "245", "Or Tor Kor market"),
    ("2026-11-10", "cash", "expense", "Fuel", "150", "PTT"),
    ("2026-11-11", "cash", "expense", "Taxi & Grab", "40", "Motorbike taxi"),
    # --- Everyday card (statement 25th) ---
    ("2026-09-03", "everyday", "expense", "Phone & internet", "599", "AIS postpaid"),
    ("2026-09-05", "everyday", "expense", "Groceries", "1,284.50", "Lotus's"),
    ("2026-09-07", "everyday", "expense", "Shopping", "459", "Shopee"),
    ("2026-09-09", "everyday", "expense", "Taxi & Grab", "156", "Grab"),
    ("2026-09-10", "everyday", "expense", "Coffee & snacks", "165", "Starbucks"),
    ("2026-09-12", "everyday", "expense", "Groceries", "892.75", "Tops"),
    ("2026-09-14", "everyday", "expense", "Food & drink", "289", "GrabFood"),
    ("2026-09-15", "everyday", "expense", "Shopping", "1,890", "Lazada – air purifier filter"),
    ("2026-09-18", "everyday", "expense", "Taxi & Grab", "212", "Grab"),
    ("2026-09-19", "everyday", "expense", "Groceries", "1,563.25", "Big C"),
    ("2026-09-21", "everyday", "expense", "Food & drink", "1,120", "Sizzler"),
    ("2026-09-23", "everyday", "expense", "Shopping", "2,349", "Shopee"),
    ("2026-09-24", "everyday", "expense", "Coffee & snacks", "185", "Starbucks"),
    ("2026-09-28", "everyday", "expense", "Taxi & Grab", "178", "Grab"),
    ("2026-10-02", "everyday", "expense", "Groceries", "1,456.75", "Lotus's"),
    ("2026-10-03", "everyday", "expense", "Phone & internet", "599", "AIS postpaid"),
    ("2026-10-06", "everyday", "expense", "Shopping", "1,299", "Shopee – sneakers"),
    ("2026-10-08", "everyday", "expense", "Food & drink", "315", "GrabFood"),
    ("2026-10-10", "everyday", "expense", "Groceries", "1,032.50", "Tops"),
    ("2026-10-11", "everyday", "expense", "Coffee & snacks", "175", "Starbucks"),
    ("2026-10-13", "everyday", "income", "Shopping", "1,299", "Refund: Shopee sneakers (wrong size)"),
    ("2026-10-14", "everyday", "expense", "Taxi & Grab", "245", "Grab"),
    ("2026-10-17", "everyday", "expense", "Groceries", "1,248", "Big C"),
    ("2026-10-18", "everyday", "expense", "Food & drink", "864", "MK Restaurants"),
    ("2026-10-21", "everyday", "expense", "Shopping", "3,590", "Lazada – rice cooker"),
    ("2026-10-22", "everyday", "expense", "Taxi & Grab", "134", "Grab"),
    ("2026-10-24", "everyday", "expense", "Coffee & snacks", "155", "Starbucks"),
    ("2026-10-27", "everyday", "expense", "Groceries", "1,388.25", "Lotus's"),
    ("2026-10-30", "everyday", "expense", "Food & drink", "342", "GrabFood"),
    ("2026-11-01", "everyday", "expense", "Groceries", "1,642.75", "Big C"),
    ("2026-11-02", "everyday", "expense", "Taxi & Grab", "189", "Grab"),
    ("2026-11-03", "everyday", "expense", "Phone & internet", "599", "AIS postpaid"),
    ("2026-11-04", "everyday", "expense", "Coffee & snacks", "175", "Starbucks"),
    ("2026-11-05", "everyday", "expense", "Food & drink", "289", "GrabFood"),
    ("2026-11-06", "everyday", "expense", "Groceries", "1,215.50", "Tops"),
    ("2026-11-07", "everyday", "expense", "Taxi & Grab", "226", "Grab"),
    ("2026-11-08", "everyday", "expense", "Coffee & snacks", "295", "After You"),
    ("2026-11-09", "everyday", "expense", "Taxi & Grab", "98", "Grab"),
    ("2026-11-10", "everyday", "expense", "Groceries", "987.25", "Lotus's"),
    ("2026-11-11", "everyday", "expense", "Shopping", "2,190", "Shopee 11.11 sale"),
    ("2026-11-11", "everyday", "expense", "Coffee & snacks", "185", "Starbucks"),
    # --- Travel card (statement 5th, paid in full) ---
    ("2026-09-10", "travel", "expense", "Entertainment & travel", "419", "Netflix"),
    ("2026-09-20", "travel", "expense", "Entertainment & travel", "3,240", "AirAsia BKK–CNX"),
    ("2026-10-10", "travel", "expense", "Entertainment & travel", "419", "Netflix"),
    ("2026-10-15", "travel", "expense", "Shopping", "1,590", "Uniqlo"),
    ("2026-10-22", "travel", "expense", "Entertainment & travel", "480", "Major Cineplex"),
    ("2026-10-25", "travel", "income", "Shopping", "590", "Refund: Uniqlo return"),
    ("2026-11-03", "travel", "expense", "Health", "356", "Watsons"),
    ("2026-11-10", "travel", "expense", "Entertainment & travel", "419", "Netflix"),
]

# A USD purchase: what the card charged in baht, and the original amount.
FOREIGN = [
    ("2026-10-02", "travel", "Entertainment & travel", "3,412.84", ("96.50", "USD"), "Agoda – Chiang Mai hotel", "2026-10-05"),
]

# Money moving between your own accounts: (date, from, to, amount, note, to_kind).
# `to_kind` is the receiving leg's kind: a transfer, or a payment on a card / loan.
MOVES = [
    ("2026-09-02", "bank", "cash", "3,000", "ATM withdrawal", "transfer"),
    ("2026-09-27", "bank", "cash", "3,000", "ATM withdrawal", "transfer"),
    ("2026-10-24", "bank", "cash", "3,000", "ATM withdrawal", "transfer"),
    ("2026-10-03", "cashcard", "cash", "15,000", "Cash card withdrawal", "transfer"),
    # Everyday card: September statement only partly paid, so October shows interest.
    ("2026-10-14", "bank", "everyday", "10,000", "Pay Everyday card", "payment"),
    # Cash card: a bit over the minimum.
    ("2026-11-04", "bank", "cashcard", "2,000", "Pay Cash card", "payment"),
    # Loans (November's personal loan payment is left unpaid on purpose).
    ("2026-09-04", "bank", "ploan", "5,000", "Personal loan installment", "payment"),
    ("2026-10-05", "bank", "ploan", "5,000", "Personal loan installment", "payment"),
    ("2026-09-15", "bank", "moto", "1,437.50", "Motorbike installment", "payment"),
    ("2026-10-15", "bank", "moto", "1,437.50", "Motorbike installment", "payment"),
]

# Loan payments from before the demo window (no bank side recorded).
EARLY_LOAN_PAYMENTS = [
    ("2026-07-03", "ploan", "5,000"),
    ("2026-08-05", "ploan", "5,000"),
] + [(f"2026-{m:02d}-15", "moto", "1,437.50") for m in range(1, 9)]

# Travel card: each statement paid in full on this date (amount worked out below).
TRAVEL_PAYMENTS = [("2026-10-05", "2026-10-20"), ("2026-11-05", "2026-11-10")]

# Monthly limits (baht) for both months. Nov: Coffee over, Groceries near the limit,
# Taxi comfortably under; Rent, Family, Phone, BTS, Health are left unbudgeted.
BUDGETS = {
    "Coffee & snacks": "800",
    "Groceries": "4,500",
    "Taxi & Grab": "1,500",
    "Food & drink": "3,000",
    "Shopping": "3,000",
    "Bills & utilities": "2,500",
    "Entertainment & travel": "2,000",
}
BUDGET_MONTHS = ["2026-10-01", "2026-11-01"]


# --- Seeding -------------------------------------------------------------------


class Seeder:
    def __init__(self, api, user_id):
        self.api = api
        self.user_id = user_id
        self.versions = {}
        self.counts = {}

    def id(self, key):
        return str(uuid.uuid5(NAMESPACE, f"{self.user_id}:{key}"))

    def load(self, resource):
        items = self.api.call("GET", f"/{resource}")["items"]
        for r in items:
            self.versions[r["id"]] = r["version"]
        return items

    def put(self, resource, rid, data):
        body = dict(data)
        if rid in self.versions:
            body["base_version"] = self.versions[rid]
        rec = self.api.call("PUT", f"/{resource}/{rid}", body)
        self.versions[rid] = rec["version"]
        self.counts[resource] = self.counts.get(resource, 0) + 1
        return rec


def seed(api, user_id):
    s = Seeder(api, user_id)
    existing_categories = s.load("categories")
    for res in ("accounts", "transactions", "installment-plans", "budgets"):
        s.load(res)

    # Categories: reuse ones the user already has with the same English name and kind
    # (left as they are); create the rest with our own ids.
    cat = {}
    by_name = {(c["kind"], c.get("name_en")): c["id"] for c in existing_categories}

    def category(kind, th, en, parent):
        ours = s.id(f"category:{en}")
        cid = by_name.get((kind, en), ours)
        if cid == ours:
            s.put("categories", cid, {"parent_id": parent, "name_th": th, "name_en": en, "kind": kind})
        cat[en] = cid
        return cid

    for kind, th, en, children in STARTER:
        parent = category(kind, th, en, None)
        for cth, cen in children:
            category(kind, cth, cen, parent)

    acct = {}
    for key, a in ACCOUNTS.items():
        acct[key] = s.id(f"account:{key}")
        s.put("accounts", acct[key], {**a, "currency": THB, "closed_on": None})

    for key, p in PLANS.items():
        s.put(
            "installment-plans",
            s.id(f"plan:{key}"),
            {
                "card_account_id": acct[p["card"]],
                "principal": money(p["principal"]),
                "months": p["months"],
                "monthly_rate_bp": p["monthly_rate_bp"],
                "first_post_on": p["first_post_on"],
                "closed_on": None,
                "rounding": rounding(),
                "remainder": "last",
            },
        )

    card_like = {k for k, a in ACCOUNTS.items() if a["kind"] in ("card", "revolving")}
    seen = {}
    txns = []  # (account key, data) for the statement maths below

    def tx(key, account, kind, on, amount, category=None, note=None, posted=None, orig=None, peer=None):
        n = seen.get(key, 0)
        seen[key] = n + 1
        rid = s.id(f"tx:{key}:{n}")
        on_d = d(on)
        if posted is None and account in card_like:
            posted = posting(on_d).isoformat()
        data = {
            "account_id": acct[account],
            "category_id": cat[category] if category else None,
            "occurred_on": on,
            "posted_on": posted,
            "amount": money(amount),
            "orig_amount": orig,
            "kind": kind,
            "transfer_peer_id": peer,
            "installment_plan_id": None,
            "note": note,
        }
        s.put("transactions", rid, data)
        txns.append((account, data))
        return rid

    for on, account, kind, category, amount, note in SPENDING:
        tx(f"{on}:{account}:{note}", account, kind, on, amount, category, note)

    for on, account, category, amount, (orig, cur), note, posted in FOREIGN:
        tx(f"{on}:{account}:{note}", account, "expense", on, amount, category, note,
           posted=posted, orig=money(orig, cur))

    for on, src, dst, amount, note, to_kind in MOVES:
        key = f"{on}:{src}>{dst}:{note}"
        out_id, in_id = s.id(f"tx:{key}:out"), s.id(f"tx:{key}:in")
        both = to_kind == "transfer"
        tx(f"{key}:out", src, "transfer", on, amount, note=note, posted=on, peer=in_id if both else None)
        tx(f"{key}:in", dst, to_kind, on, amount, note=note, posted=on, peer=out_id if both else None)

    for on, loan, amount in EARLY_LOAN_PAYMENTS:
        tx(f"{on}:{loan}:early", loan, "payment", on, amount, note="Installment")

    # Travel card: pay each statement's closing balance in full (no interest thanks to the grace period).
    for stmt, pay_on in TRAVEL_PAYMENTS:
        end = d(stmt)
        total = 0
        for account, t in txns:
            if account != "travel":
                continue
            p = d(t["posted_on"])
            if p <= end:
                sign = -1 if t["kind"] in ("income", "payment") else 1
                total += sign * t["amount"]["minor"]
        amount = f"{total // 100}.{total % 100:02d}"
        key = f"{pay_on}:bank>travel:Pay Travel card in full"
        tx(f"{key}:out", "bank", "transfer", pay_on, amount, note="Pay Travel card in full", posted=pay_on)
        tx(f"{key}:in", "travel", "payment", pay_on, amount, note="Pay Travel card in full", posted=pay_on)

    for month in BUDGET_MONTHS:
        for name, limit in BUDGETS.items():
            s.put(
                "budgets",
                s.id(f"budget:{month}:{name}"),
                {"category_id": cat[name], "period_month": month, "limit": money(limit)},
            )
    return s


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--api", default="http://localhost:8080")
    who = ap.add_mutually_exclusive_group()
    who.add_argument("--email", default="demo@billo.dev")
    who.add_argument("--token", help="bearer token of an existing session, or the whole billo.session value")
    args = ap.parse_args()

    token = args.token
    if token and token.lstrip().startswith("{"):
        token = json.loads(token)["token"]  # the whole `billo.session` value
    api = Api(args.api, token)
    if args.token:
        me = api.call("GET", "/me")
        user_id, email = me["user_id"], me.get("email") or "this account"
    else:
        login = api.call("POST", "/auth/dev-login", {"email": args.email})
        api.token = login["token"]
        user_id, email = login["user_id"], login.get("email") or args.email

    s = seed(api, user_id)
    print(f"Seeded demo data for {email} ({user_id}):")
    for res in ("categories", "accounts", "installment-plans", "transactions", "budgets"):
        print(f"  {res:<18} {s.counts.get(res, 0)}")
    if api.conflicts:
        print(f"  ({api.conflicts} writes replaced newer server versions)")
    if args.token:
        print("Open the app signed in as that user; it picks the data up on the next sync.")
    else:
        print(f"Open http://localhost:8081 and sign in with the email {email} (dev login).")


if __name__ == "__main__":
    main()
