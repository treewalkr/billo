//! `/api/v1/sync/push` and `/api/v1/sync/pull`. Skipped when `DATABASE_URL` is unset.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

use common::{Res, TestApp, app};

fn account_payload(name: &str) -> Value {
    json!({ "kind": "cash", "name": name, "currency": "THB" })
}

fn put(entity: &str, id: Uuid, base: Option<i64>, payload: Value) -> (Uuid, Value) {
    let mid = Uuid::now_v7();
    (
        mid,
        json!({
            "mutation_id": mid, "entity": entity, "entity_id": id,
            "base_version": base, "op": "put", "payload": payload,
        }),
    )
}

fn del(entity: &str, id: Uuid) -> (Uuid, Value) {
    let mid = Uuid::now_v7();
    (
        mid,
        json!({ "mutation_id": mid, "entity": entity, "entity_id": id, "op": "delete" }),
    )
}

async fn push(t: &TestApp, token: &str, mutations: Vec<Value>) -> Res {
    t.call(
        Method::POST,
        "/api/v1/sync/push",
        Some(token),
        Some(json!({ "mutations": mutations })),
    )
    .await
}

async fn pull(t: &TestApp, token: &str, since: i64, limit: u32) -> Res {
    let res = t
        .get(
            &format!("/api/v1/sync/pull?since={since}&limit={limit}"),
            token,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    res
}

fn results(res: &Res) -> &Vec<Value> {
    res.body["results"].as_array().unwrap()
}

fn changes(res: &Res) -> &Vec<Value> {
    res.body["changes"].as_array().unwrap()
}

#[tokio::test]
async fn push_applies_and_replay_is_idempotent() {
    let Some(t) = app().await else { return };
    let (token, user_id) = t.login().await;
    let acc = Uuid::now_v7();
    let (m1, put1) = put("account", acc, None, account_payload("  Wallet  "));
    let res = push(&t, &token, vec![put1.clone()]).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let r = &results(&res)[0];
    assert_eq!(r["mutation_id"], json!(m1));
    assert_eq!(r["ok"], true);
    assert_eq!(r["conflict"], false);
    assert_eq!(r["version"], 1);
    // The state is the stored (normalised) record.
    assert_eq!(r["state"]["record"]["name"], "Wallet");
    assert_eq!(r["state"]["deleted"], false);

    // Replaying the same mutation returns the original result and writes nothing,
    // even after the record changed in between.
    let (_, put2) = put("account", acc, Some(1), account_payload("Purse"));
    push(&t, &token, vec![put2]).await;
    let replay = push(&t, &token, vec![put1]).await;
    assert_eq!(results(&replay)[0], *r);
    assert_eq!(
        t.get(&format!("/api/v1/accounts/{acc}"), &token).await.body["name"],
        "Purse"
    );
    assert_eq!(t.change_log(user_id, acc).await.len(), 2);
}

#[tokio::test]
async fn one_bad_mutation_does_not_fail_the_batch() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let (other, _) = t.login().await;
    let good = Uuid::now_v7();
    let bad = Uuid::now_v7();
    let foreign = Uuid::now_v7();
    // An id owned by another user.
    push(
        &t,
        &other,
        vec![put("account", foreign, None, account_payload("x")).1],
    )
    .await;

    let res = push(
        &t,
        &token,
        vec![
            put("account", good, None, account_payload("Wallet")).1,
            put(
                "account",
                bad,
                None,
                json!({ "kind": "cash", "name": "", "currency": "THB" }),
            )
            .1,
            put("account", Uuid::now_v7(), None, json!({ "kind": "nope" })).1,
            del("account", Uuid::now_v7()).1,
            put("account", foreign, None, account_payload("mine")).1,
            put(
                "category",
                Uuid::now_v7(),
                None,
                json!({ "name_en": "Food", "kind": "expense" }),
            )
            .1,
        ],
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let r = results(&res);
    assert_eq!(r.len(), 6);
    assert_eq!(r[0]["ok"], true);
    assert_eq!(r[1]["ok"], false);
    assert_eq!(r[1]["error"]["code"], "validation");
    assert_eq!(r[1]["state"], Value::Null);
    assert_eq!(r[2]["error"]["code"], "validation");
    assert_eq!(r[3]["error"]["code"], "not_found");
    assert_eq!(r[4]["error"]["code"], "conflict");
    // The other user's record is not revealed.
    assert_eq!(r[4]["state"], Value::Null);
    assert_eq!(r[5]["ok"], true);
    assert_eq!(
        t.get(&format!("/api/v1/accounts/{bad}"), &token)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // A rejected mutation isn't recorded: once fixed data exists it can't be replayed
    // into success, but a rejection with the server's current state comes back.
    let (mid, upd) = put(
        "account",
        good,
        Some(1),
        json!({ "kind": "cash", "name": "", "currency": "THB" }),
    );
    let res = push(&t, &token, vec![upd.clone()]).await;
    let r = &results(&res)[0];
    assert_eq!(r["mutation_id"], json!(mid));
    assert_eq!(r["error"]["code"], "validation");
    assert_eq!(r["state"]["version"], 1);
    assert_eq!(r["state"]["record"]["name"], "Wallet");
}

#[tokio::test]
async fn stale_base_version_is_applied_and_flagged() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let acc = Uuid::now_v7();
    push(
        &t,
        &token,
        vec![put("account", acc, None, account_payload("A")).1],
    )
    .await;
    push(
        &t,
        &token,
        vec![put("account", acc, Some(1), account_payload("B")).1],
    )
    .await;
    let res = push(
        &t,
        &token,
        vec![put("account", acc, Some(1), account_payload("C")).1],
    )
    .await;
    let r = &results(&res)[0];
    assert_eq!(r["ok"], true);
    assert_eq!(r["conflict"], true);
    assert_eq!(r["version"], 3);
    assert_eq!(r["state"]["record"]["name"], "C");
    // Up to date: no flag.
    let res = push(
        &t,
        &token,
        vec![put("account", acc, Some(3), account_payload("D")).1],
    )
    .await;
    assert_eq!(results(&res)[0]["conflict"], false);
}

#[tokio::test]
async fn push_limits() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let many: Vec<Value> = (0..201)
        .map(|_| put("account", Uuid::now_v7(), None, account_payload("x")).1)
        .collect();
    let res = push(&t, &token, many).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    let huge = "x".repeat(2 * 1024 * 1024);
    let res = push(
        &t,
        &token,
        vec![put("account", Uuid::now_v7(), None, account_payload(&huge)).1],
    )
    .await;
    assert!(res.status.is_client_error(), "{}", res.status);
    let res = t
        .call(
            Method::POST,
            "/api/v1/sync/push",
            None,
            Some(json!({ "mutations": [] })),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pull_pages_collapses_and_returns_tombstones() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    push(
        &t,
        &token,
        vec![
            put("account", a, None, account_payload("A1")).1,
            put("account", b, None, account_payload("B1")).1,
            put("account", a, Some(1), account_payload("A2")).1,
            put(
                "category",
                c,
                None,
                json!({ "name_en": "Food", "kind": "expense" }),
            )
            .1,
            del("account", b).1,
        ],
    )
    .await;

    // Everything in one page: each record once, at its latest change, in seq order.
    let all = pull(&t, &token, 0, 100).await;
    let ch = changes(&all);
    assert_eq!(all.body["has_more"], false);
    assert_eq!(ch.len(), 3);
    let order: Vec<&str> = ch.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(order, [a.to_string(), c.to_string(), b.to_string()]);
    assert_eq!(ch[0]["record"]["name"], "A2");
    assert_eq!(ch[0]["version"], 2);
    assert_eq!(ch[1]["entity"], "category");
    assert_eq!(ch[2]["deleted"], true);
    assert_eq!(ch[2]["record"], Value::Null);
    assert_eq!(ch[2]["version"], 2);
    let seqs: Vec<i64> = ch.iter().map(|c| c["seq"].as_i64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
    let end = all.body["next_since"].as_i64().unwrap();
    assert_eq!(end, *seqs.last().unwrap());

    // Pages of 2: the cursor walks all five feed rows without gaps.
    let p1 = pull(&t, &token, 0, 2).await;
    assert_eq!(p1.body["has_more"], true);
    assert_eq!(changes(&p1).len(), 2);
    let s1 = p1.body["next_since"].as_i64().unwrap();
    let p2 = pull(&t, &token, s1, 2).await;
    assert_eq!(p2.body["has_more"], true);
    // A1 and A2 are in different pages; the record's current state comes both times.
    assert_eq!(changes(&p1)[0]["record"]["name"], "A2");
    let s2 = p2.body["next_since"].as_i64().unwrap();
    let p3 = pull(&t, &token, s2, 2).await;
    assert_eq!(p3.body["has_more"], false);
    assert_eq!(changes(&p3).len(), 1);
    assert_eq!(changes(&p3)[0]["deleted"], true);
    assert_eq!(p3.body["next_since"].as_i64().unwrap(), end);

    // Nothing new: empty page, cursor unchanged.
    let none = pull(&t, &token, end, 100).await;
    assert_eq!(changes(&none).len(), 0);
    assert_eq!(none.body["next_since"].as_i64().unwrap(), end);
    assert_eq!(none.body["has_more"], false);

    // Changes made through the CRUD routes appear in the feed too.
    t.put(
        &format!("/api/v1/accounts/{a}"),
        &token,
        json!({ "base_version": 2, "kind": "cash", "name": "A3", "currency": "THB" }),
    )
    .await;
    let after = pull(&t, &token, end, 100).await;
    assert_eq!(changes(&after).len(), 1);
    assert_eq!(changes(&after)[0]["record"]["name"], "A3");
    assert!(changes(&after)[0]["seq"].as_i64().unwrap() > end);
}

#[tokio::test]
async fn pull_and_push_are_isolated_per_user() {
    let Some(t) = app().await else { return };
    let (alice, _) = t.login().await;
    let (bob, _) = t.login().await;
    let a = Uuid::now_v7();
    let (mid, m) = put("account", a, None, account_payload("Alice's"));
    push(&t, &alice, vec![m.clone()]).await;
    let res = pull(&t, &bob, 0, 100).await;
    assert_eq!(changes(&res).len(), 0);
    // Bob replaying Alice's mutation id is a new mutation for Bob: the id is taken.
    let res = push(&t, &bob, vec![m]).await;
    let r = &results(&res)[0];
    assert_eq!(r["mutation_id"], json!(mid));
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "conflict");
    // Deleting Alice's record as Bob: not found.
    let res = push(&t, &bob, vec![del("account", a).1]).await;
    assert_eq!(results(&res)[0]["error"]["code"], "not_found");
    assert_eq!(changes(&pull(&t, &alice, 0, 100).await).len(), 1);
}

#[tokio::test]
async fn pull_rejects_bad_query() {
    let Some(t) = app().await else { return };
    let (token, _) = t.login().await;
    let res = t.get("/api/v1/sync/pull?since=abc", &token).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    let res = t.get("/api/v1/sync/pull", &token).await;
    assert_eq!(res.status, StatusCode::OK);
}
