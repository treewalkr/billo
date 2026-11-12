//! `/api/v1/accounts` and `/api/v1/categories`. Skipped when `DATABASE_URL` is unset.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

use common::app;

fn card_rules() -> Value {
    json!({
        "v": 1,
        "card": {
            "statement_day": 25,
            "due": { "days_after_statement": 20 },
            "due_shift": "next",
            "credit_limit_minor": 5_000_000,
            "min_payment": { "percent_bp": 800, "floor_minor": 100_000 },
            "interest": {
                "annual_rate_bp": 1600,
                "start": "transaction_date",
                "grace_if_paid_in_full": true
            }
        }
    })
}

fn card() -> Value {
    json!({ "kind": "card", "name": "My card", "currency": "THB", "rules": card_rules() })
}

#[tokio::test]
async fn account_crud_versions_and_change_log() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let id = Uuid::now_v7();
    let uri = format!("/api/v1/accounts/{id}");

    // Create.
    let res = t.put(&uri, &token, card()).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["id"], id.to_string());
    assert_eq!(res.body["version"], 1);
    // Defaults are filled in and stored explicitly.
    assert_eq!(res.body["rules"]["card"]["statement_day"], 25);
    assert_eq!(
        res.body["rules"]["card"]["interest"]["rounding"],
        json!({ "step": 1, "mode": "half_up" })
    );
    assert!(!res.conflict_flag());

    // Same PUT again (a retry): unchanged, no new version.
    let res = t.put(&uri, &token, card()).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["version"], 1);
    assert!(!res.conflict_flag());

    // Read.
    let res = t.get(&uri, &token).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["name"], "My card");

    // Update with the right base version.
    let mut body = card();
    body["name"] = json!("Renamed");
    body["base_version"] = json!(1);
    let res = t.put(&uri, &token, body.clone()).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["version"], 2);
    assert_eq!(res.body["name"], "Renamed");
    assert!(!res.conflict_flag());

    // Stale base version: applied anyway, flagged.
    body["name"] = json!("Stale write");
    let res = t.put(&uri, &token, body).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["version"], 3);
    assert_eq!(res.body["name"], "Stale write");
    assert!(res.conflict_flag());

    // List.
    let res = t.get("/api/v1/accounts", &token).await;
    let items = res.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id.to_string());

    // Delete hides it; deleting again is a no-op.
    assert_eq!(t.delete(&uri, &token).await.status, StatusCode::NO_CONTENT);
    assert_eq!(t.delete(&uri, &token).await.status, StatusCode::NO_CONTENT);
    assert_eq!(t.get(&uri, &token).await.status, StatusCode::NOT_FOUND);
    let res = t.get("/api/v1/accounts", &token).await;
    assert_eq!(res.body["items"], json!([]));

    assert_eq!(
        t.change_log(user_id, id).await,
        vec![
            ("account".into(), 1, "create".into()),
            ("account".into(), 2, "update".into()),
            ("account".into(), 3, "update".into()),
            ("account".into(), 4, "delete".into()),
        ]
    );

    // PUT after delete restores it.
    let mut body = card();
    body["base_version"] = json!(4);
    let res = t.put(&uri, &token, body).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["version"], 5);
    assert_eq!(t.get(&uri, &token).await.status, StatusCode::OK);
}

#[tokio::test]
async fn accounts_are_isolated_between_users() {
    let Some(t) = app().await else { return };
    let (alice, _) = t.login().await;
    let (bob, bob_id) = t.login().await;
    let id = Uuid::now_v7();
    let uri = format!("/api/v1/accounts/{id}");
    assert_eq!(
        t.put(&uri, &alice, card()).await.status,
        StatusCode::CREATED
    );

    assert_eq!(t.get(&uri, &bob).await.status, StatusCode::NOT_FOUND);
    assert_eq!(t.delete(&uri, &bob).await.status, StatusCode::NOT_FOUND);
    let res = t.get("/api/v1/accounts", &bob).await;
    assert_eq!(res.body["items"], json!([]));

    // Bob can't take over Alice's id.
    let res = t.put(&uri, &bob, card()).await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.error_code(), "conflict");
    assert!(t.change_log(bob_id, id).await.is_empty());
    assert_eq!(t.get(&uri, &alice).await.body["name"], "My card");
}

#[tokio::test]
async fn account_validation() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let uri = format!("/api/v1/accounts/{}", Uuid::now_v7());

    let mut bad_day = card_rules();
    bad_day["card"]["statement_day"] = json!(32);
    let mut bad_percent = card_rules();
    bad_percent["card"]["min_payment"]["percent_bp"] = json!(0);
    let mut bad_rounding = card_rules();
    bad_rounding["card"]["interest"]["rounding"] = json!({ "step": 0, "mode": "up" });
    let cases = [
        json!({ "kind": "cash", "name": "x", "currency": "XYZ" }),
        json!({ "kind": "cash", "name": "x", "currency": "thb" }),
        json!({ "kind": "cash", "name": "   ", "currency": "THB" }),
        json!({ "kind": "cash", "name": "x".repeat(101), "currency": "THB" }),
        json!({ "kind": "wallet", "name": "x", "currency": "THB" }),
        json!({ "kind": "cash", "currency": "THB" }),
        json!({ "kind": "cash", "name": "x", "currency": "THB", "rules": { "v": 2 } }),
        json!({ "kind": "cash", "name": "x", "currency": "THB", "rules": card_rules() }),
        json!({ "kind": "card", "name": "x", "currency": "THB", "rules": bad_day }),
        json!({ "kind": "card", "name": "x", "currency": "THB", "rules": bad_percent }),
        json!({ "kind": "card", "name": "x", "currency": "THB", "rules": bad_rounding }),
        json!({ "kind": "loan", "name": "x", "currency": "THB", "rules": { "v": 1, "loan": {
            "principal_minor": 0, "annual_rate_bp": 500, "months": 12,
            "start": "2026-01-01", "first_payment": "2026-02-01", "method": "flat" } } }),
        json!({ "kind": "cash", "name": "x", "currency": "THB",
                "opened_on": "2026-02-01", "closed_on": "2026-01-01" }),
    ];
    for body in cases {
        let res = t.put(&uri, &token, body.clone()).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(res.error_code(), "validation", "{body}");
    }
    // Nothing was written.
    assert_eq!(t.get(&uri, &token).await.status, StatusCode::NOT_FOUND);

    // Malformed JSON is a validation error too; malformed ids are not found.
    let res = t
        .call(
            Method::PUT,
            &uri,
            Some(&token),
            Some(json!("not an object")),
        )
        .await;
    assert_eq!(res.error_code(), "validation");
    let res = t.get("/api/v1/accounts/not-a-uuid", &token).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.error_code(), "not_found");

    // A valid loan.
    let res = t
        .put(
            &uri,
            &token,
            json!({ "kind": "loan", "name": "Car", "currency": "THB", "rules": { "v": 1, "loan": {
                "principal_minor": 50_000_000, "annual_rate_bp": 299, "months": 48,
                "start": "2026-01-10", "first_payment": "2026-02-10", "method": "flat" } } }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["rules"]["loan"]["start"], "2026-01-10");
}

#[tokio::test]
async fn resources_require_auth() {
    let Some(t) = app().await else { return };
    let id = Uuid::now_v7();
    for (method, uri) in [
        (Method::GET, "/api/v1/accounts".to_owned()),
        (Method::GET, format!("/api/v1/accounts/{id}")),
        (Method::PUT, format!("/api/v1/accounts/{id}")),
        (Method::DELETE, format!("/api/v1/accounts/{id}")),
        (Method::GET, "/api/v1/categories".to_owned()),
    ] {
        let res = t.call(method, &uri, None, Some(card())).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(res.error_code(), "unauthorized");
    }
}

#[tokio::test]
async fn category_crud_and_parent() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let (other, _) = t.login().await;
    let parent = Uuid::now_v7();
    let child = Uuid::now_v7();

    let res = t
        .put(
            &format!("/api/v1/categories/{parent}"),
            &token,
            json!({ "name_th": " อาหาร ", "name_en": "", "kind": "expense" }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["name_th"], "อาหาร");
    assert_eq!(res.body["name_en"], Value::Null);

    let child_body = json!({ "parent_id": parent, "name_en": "Coffee", "kind": "expense" });
    let res = t
        .put(
            &format!("/api/v1/categories/{child}"),
            &token,
            child_body.clone(),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    assert_eq!(res.body["parent_id"], parent.to_string());

    // Another user can't hang a category under it.
    let res = t
        .put(
            &format!("/api/v1/categories/{}", Uuid::now_v7()),
            &other,
            child_body,
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Names are required.
    let res = t
        .put(
            &format!("/api/v1/categories/{}", Uuid::now_v7()),
            &token,
            json!({ "name_th": " ", "kind": "income" }),
        )
        .await;
    assert_eq!(res.error_code(), "validation");

    let res = t.get("/api/v1/categories", &token).await;
    assert_eq!(res.body["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        t.delete(&format!("/api/v1/categories/{child}"), &token)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let res = t.get("/api/v1/categories", &token).await;
    assert_eq!(res.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        t.change_log(user_id, child).await,
        vec![
            ("category".into(), 1, "create".into()),
            ("category".into(), 2, "delete".into()),
        ]
    );
}
