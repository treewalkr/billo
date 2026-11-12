//! `/api/v1/transactions`, `/api/v1/installment-plans` and `/api/v1/budgets`.
//! Skipped when `DATABASE_URL` is unset.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{TestApp, app};

async fn account(t: &TestApp, token: &str, kind: &str, rules: Value) -> Uuid {
    let id = Uuid::now_v7();
    let res = t
        .put(
            &format!("/api/v1/accounts/{id}"),
            token,
            json!({ "kind": kind, "name": "acct", "currency": "THB", "rules": rules }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    id
}

async fn category(t: &TestApp, token: &str) -> Uuid {
    let id = Uuid::now_v7();
    let res = t
        .put(
            &format!("/api/v1/categories/{id}"),
            token,
            json!({ "name_en": "Food", "kind": "expense" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    id
}

fn expense(account: Uuid, on: &str, minor: i64) -> Value {
    json!({
        "account_id": account,
        "occurred_on": on,
        "amount": { "minor": minor, "currency": "THB" },
        "kind": "expense",
        "note": "lunch"
    })
}

fn ids(res: &common::Res) -> Vec<String> {
    res.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn transaction_crud_filters_and_change_log() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let cash = account(&t, &token, "cash", json!({ "v": 1 })).await;
    let bank = account(&t, &token, "bank", json!({ "v": 1 })).await;
    let food = category(&t, &token).await;

    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    let mut body = expense(cash, "2026-11-01", 12_050);
    body["category_id"] = json!(food);
    let res = t
        .put(&format!("/api/v1/transactions/{a}"), &token, body)
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(
        res.body["amount"],
        json!({ "minor": 12_050, "currency": "THB" })
    );
    assert_eq!(res.body["category_id"], food.to_string());
    assert_eq!(res.body["orig_amount"], Value::Null);
    for (id, acct, on) in [(b, cash, "2026-11-05"), (c, bank, "2026-10-20")] {
        let res = t
            .put(
                &format!("/api/v1/transactions/{id}"),
                &token,
                expense(acct, on, 100),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    }

    // Newest first; filters combine.
    let all = t.get("/api/v1/transactions", &token).await;
    assert_eq!(ids(&all), [b, a, c].map(|i| i.to_string()));
    let res = t
        .get(&format!("/api/v1/transactions?account_id={cash}"), &token)
        .await;
    assert_eq!(ids(&res), [b, a].map(|i| i.to_string()));
    let res = t
        .get("/api/v1/transactions?from=2026-11-01&to=2026-11-04", &token)
        .await;
    assert_eq!(ids(&res), [a.to_string()]);
    let res = t.get("/api/v1/transactions?from=nope", &token).await;
    assert_eq!(res.error_code(), "validation");

    // Update with a foreign original amount.
    let mut body = expense(cash, "2026-11-01", 36_000);
    body["orig_amount"] = json!({ "minor": 1_000, "currency": "USD" });
    body["base_version"] = json!(1);
    let res = t
        .put(&format!("/api/v1/transactions/{a}"), &token, body)
        .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["version"], 2);
    assert_eq!(res.body["orig_amount"]["currency"], "USD");
    assert!(!res.conflict_flag());

    assert_eq!(
        t.delete(&format!("/api/v1/transactions/{a}"), &token)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let res = t.get("/api/v1/transactions", &token).await;
    assert_eq!(ids(&res), [b, c].map(|i| i.to_string()));
    assert_eq!(
        t.change_log(user_id, a).await,
        vec![
            ("transaction".into(), 1, "create".into()),
            ("transaction".into(), 2, "update".into()),
            ("transaction".into(), 3, "delete".into()),
        ]
    );
}

#[tokio::test]
async fn transaction_validation_and_isolation() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let (other, _) = t.login().await;
    let cash = account(&t, &token, "cash", json!({ "v": 1 })).await;
    let others_cash = account(&t, &other, "cash", json!({ "v": 1 })).await;
    let others_food = category(&t, &other).await;
    let uri = format!("/api/v1/transactions/{}", Uuid::now_v7());

    let mut usd = expense(cash, "2026-11-01", 100);
    usd["amount"]["currency"] = json!("USD");
    let mut foreign_category = expense(cash, "2026-11-01", 100);
    foreign_category["category_id"] = json!(others_food);
    let mut peer_on_expense = expense(cash, "2026-11-01", 100);
    peer_on_expense["transfer_peer_id"] = json!(Uuid::now_v7());
    let mut zero_adjust = expense(cash, "2026-11-01", 0);
    zero_adjust["kind"] = json!("interest_adjust");
    let mut bad_orig = expense(cash, "2026-11-01", 100);
    bad_orig["orig_amount"] = json!({ "minor": -5, "currency": "USD" });
    let mut long_note = expense(cash, "2026-11-01", 100);
    long_note["note"] = json!("x".repeat(2_001));
    let mut bad_date = expense(cash, "2026-11-01", 100);
    bad_date["occurred_on"] = json!("2026-13-01");
    for body in [
        expense(cash, "2026-11-01", 0),
        expense(cash, "2026-11-01", -100),
        expense(others_cash, "2026-11-01", 100),
        expense(Uuid::now_v7(), "2026-11-01", 100),
        usd,
        foreign_category,
        peer_on_expense,
        zero_adjust,
        bad_orig,
        long_note,
        bad_date,
    ] {
        let res = t.put(&uri, &token, body.clone()).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(res.error_code(), "validation", "{body}");
    }

    // A negative interest adjustment is allowed.
    let mut adjust = expense(cash, "2026-11-01", -250);
    adjust["kind"] = json!("interest_adjust");
    let res = t.put(&uri, &token, adjust).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);

    // Other users see nothing.
    assert_eq!(t.get(&uri, &other).await.status, StatusCode::NOT_FOUND);
    let res = t.get("/api/v1/transactions", &other).await;
    assert_eq!(res.body["items"], json!([]));
    let res = t
        .get(&format!("/api/v1/transactions?account_id={cash}"), &other)
        .await;
    assert_eq!(res.body["items"], json!([]));
}

fn card_rules() -> Value {
    json!({
        "v": 1,
        "card": {
            "statement_day": 25,
            "due": { "day_of_month": 10 },
            "due_shift": "none",
            "min_payment": { "percent_bp": 1000, "floor_minor": 50_000 },
            "interest": {
                "annual_rate_bp": 1600,
                "start": "posting_date",
                "grace_if_paid_in_full": true
            }
        }
    })
}

fn plan(card: Uuid) -> Value {
    json!({
        "card_account_id": card,
        "principal": { "minor": 1_200_000, "currency": "THB" },
        "months": 10,
        "monthly_rate_bp": 0,
        "first_post_on": "2026-11-25"
    })
}

#[tokio::test]
async fn installment_plan_crud_and_validation() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let (other, _) = t.login().await;
    let card = account(&t, &token, "card", card_rules()).await;
    let cash = account(&t, &token, "cash", json!({ "v": 1 })).await;
    let id = Uuid::now_v7();
    let uri = format!("/api/v1/installment-plans/{id}");

    let res = t.put(&uri, &token, plan(card)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["months"], 10);
    assert_eq!(res.body["remainder"], "last");
    assert_eq!(
        res.body["rounding"],
        json!({ "step": 1, "mode": "half_up" })
    );
    // Retry is idempotent.
    let res = t.put(&uri, &token, plan(card)).await;
    assert_eq!(
        (res.status, res.body["version"].clone()),
        (StatusCode::OK, json!(1))
    );

    // A portion posted by the plan links to it; only on the plan's card.
    let mut portion = expense(card, "2026-11-25", 120_000);
    portion["installment_plan_id"] = json!(id);
    let res = t
        .put(
            &format!("/api/v1/transactions/{}", Uuid::now_v7()),
            &token,
            portion,
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    let mut wrong = expense(cash, "2026-11-25", 120_000);
    wrong["installment_plan_id"] = json!(id);
    let res = t
        .put(
            &format!("/api/v1/transactions/{}", Uuid::now_v7()),
            &token,
            wrong,
        )
        .await;
    assert_eq!(res.error_code(), "validation");

    let mut zero_months = plan(card);
    zero_months["months"] = json!(0);
    let mut big_rate = plan(card);
    big_rate["monthly_rate_bp"] = json!(10_001);
    let mut usd = plan(card);
    usd["principal"]["currency"] = json!("USD");
    let mut negative = plan(card);
    negative["principal"]["minor"] = json!(-1);
    let mut early_close = plan(card);
    early_close["closed_on"] = json!("2026-01-01");
    for body in [
        zero_months,
        big_rate,
        usd,
        negative,
        early_close,
        plan(cash),
        plan(Uuid::now_v7()),
    ] {
        let res = t
            .put(
                &format!("/api/v1/installment-plans/{}", Uuid::now_v7()),
                &token,
                body.clone(),
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }

    assert_eq!(t.get(&uri, &other).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        t.put(&uri, &other, plan(card)).await.status,
        StatusCode::CONFLICT
    );
    let res = t.get("/api/v1/installment-plans", &token).await;
    assert_eq!(res.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(t.delete(&uri, &token).await.status, StatusCode::NO_CONTENT);
    let res = t.get("/api/v1/installment-plans", &token).await;
    assert_eq!(res.body["items"], json!([]));
    assert_eq!(t.change_log(user_id, id).await.len(), 2);
}

#[tokio::test]
async fn budget_crud_uniqueness_and_validation() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let (other, _) = t.login().await;
    let food = category(&t, &token).await;
    let id = Uuid::now_v7();
    let uri = format!("/api/v1/budgets/{id}");
    let budget = |month: &str, minor: i64| {
        json!({
            "category_id": food,
            "period_month": month,
            "limit": { "minor": minor, "currency": "THB" }
        })
    };

    let res = t.put(&uri, &token, budget("2026-11-01", 800_000)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["period_month"], "2026-11-01");

    // Update without base_version: applied, flagged.
    let res = t.put(&uri, &token, budget("2026-11-01", 900_000)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["version"], 2);
    assert!(res.conflict_flag());

    // One live budget per category and month.
    let dup = format!("/api/v1/budgets/{}", Uuid::now_v7());
    let res = t.put(&dup, &token, budget("2026-11-01", 1)).await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.error_code(), "conflict");
    let res = t.put(&dup, &token, budget("2026-12-01", 0)).await;
    assert_eq!(res.status, StatusCode::CREATED);

    let res = t
        .get("/api/v1/budgets?period_month=2026-12-01", &token)
        .await;
    assert_eq!(res.body["items"].as_array().unwrap().len(), 1);
    let res = t.get("/api/v1/budgets", &token).await;
    assert_eq!(res.body["items"].as_array().unwrap().len(), 2);

    for body in [
        budget("2026-11-15", 1),
        budget("2026-11-01", -1),
        json!({ "category_id": food, "period_month": "2026-10-01",
                "limit": { "minor": 1, "currency": "BTC" } }),
    ] {
        let res = t
            .put(
                &format!("/api/v1/budgets/{}", Uuid::now_v7()),
                &token,
                body.clone(),
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    // Another user's category can't be budgeted.
    let res = t
        .put(
            &format!("/api/v1/budgets/{}", Uuid::now_v7()),
            &other,
            budget("2026-11-01", 1),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(t.get(&uri, &other).await.status, StatusCode::NOT_FOUND);

    // Deleting frees the month for a new budget.
    assert_eq!(t.delete(&uri, &token).await.status, StatusCode::NO_CONTENT);
    let res = t
        .put(
            &format!("/api/v1/budgets/{}", Uuid::now_v7()),
            &token,
            budget("2026-11-01", 5),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_eq!(
        t.change_log(user_id, id)
            .await
            .into_iter()
            .map(|(_, v, op)| (v, op))
            .collect::<Vec<_>>(),
        vec![
            (1, "create".into()),
            (2, "update".into()),
            (3, "delete".into())
        ]
    );
}
