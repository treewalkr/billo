//! Sync (architecture §5):
//!
//! - `POST /api/v1/sync/push` applies a batch of queued mutations through the same
//!   repository code as the CRUD routes. Each `mutation_id` is applied at most once per
//!   user; a replay returns the stored result. One rejected mutation doesn't fail the
//!   batch.
//! - `GET /api/v1/sync/pull?since=<seq>&limit=` returns the user's change feed after
//!   `since`, joined to the records' current state (deleted ones as tombstones).

use std::collections::{BTreeMap, HashMap};

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::routing::{get, post};
use billo_api::Put;
use billo_api::sync::{
    Change, DEFAULT_PULL_LIMIT, Entity, EntityState, MAX_PULL_LIMIT, MAX_PUSH_MUTATIONS, Mutation,
    MutationOp, MutationResult, PullQuery, PullResponse, PushRequest, PushResponse,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::extract::{Json, Query};
use crate::repo::{self, Resource};
use crate::resources::accounts::Accounts;
use crate::resources::budgets::Budgets;
use crate::resources::categories::Categories;
use crate::resources::installment_plans::InstallmentPlans;
use crate::resources::transactions::Transactions;

/// Largest push body: room for [`MAX_PUSH_MUTATIONS`] generous records.
pub const PUSH_BODY_LIMIT: usize = 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sync/push",
            post(push).layer(DefaultBodyLimit::max(PUSH_BODY_LIMIT)),
        )
        .route("/sync/pull", get(pull))
}

async fn push(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<PushRequest>,
) -> ApiResult<Json<PushResponse>> {
    if req.mutations.len() > MAX_PUSH_MUTATIONS {
        return Err(ApiError::validation(format!(
            "mutations: at most {MAX_PUSH_MUTATIONS} per push"
        )));
    }
    let mut results = Vec::with_capacity(req.mutations.len());
    let mut failed = false;
    for m in req.mutations {
        if failed {
            // Later mutations may depend on the failed one: leave them for a retry.
            results.push(error_result(m.mutation_id, &ApiError::Internal, None));
            continue;
        }
        let result = match apply(&state.db, user.user_id, &m).await {
            Ok(r) => r,
            Err(e) => {
                failed = true;
                error_result(m.mutation_id, &e, None)
            }
        };
        results.push(result);
    }
    Ok(Json(PushResponse { results }))
}

fn error_result(mutation_id: Uuid, e: &ApiError, state: Option<EntityState>) -> MutationResult {
    MutationResult {
        mutation_id,
        ok: false,
        conflict: false,
        version: None,
        error: Some(e.detail()),
        state,
    }
}

/// Applies one mutation, or returns its stored result. `Err` only for failures worth
/// retrying (database down and the like); rejections are an `Ok` result with `error`.
async fn apply(db: &PgPool, user_id: Uuid, m: &Mutation) -> ApiResult<MutationResult> {
    let mut tx = db.begin().await?;
    // Claim the id first: a concurrent push of the same mutation waits here, then
    // reads the stored result.
    let claimed = sqlx::query(
        "INSERT INTO sync_mutations (user_id, mutation_id, result) VALUES ($1, $2, 'null') \
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(m.mutation_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if !claimed {
        let (sqlx::types::Json(result),): (sqlx::types::Json<MutationResult>,) = sqlx::query_as(
            "SELECT result FROM sync_mutations WHERE user_id = $1 AND mutation_id = $2",
        )
        .bind(user_id)
        .bind(m.mutation_id)
        .fetch_one(&mut *tx)
        .await?;
        return Ok(result);
    }

    match dispatch_apply(&mut tx, user_id, m).await {
        Ok((version, conflict, state)) => {
            let result = MutationResult {
                mutation_id: m.mutation_id,
                ok: true,
                conflict,
                version: Some(version),
                error: None,
                state,
            };
            sqlx::query(
                "UPDATE sync_mutations SET result = $3 WHERE user_id = $1 AND mutation_id = $2",
            )
            .bind(user_id)
            .bind(m.mutation_id)
            .bind(sqlx::types::Json(&result))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(result)
        }
        Err(ApiError::Internal) => Err(ApiError::Internal),
        Err(e) => {
            // Undo the claim (and anything half-done); a replay is checked again.
            tx.rollback().await?;
            let mut conn = db.acquire().await?;
            let state = dispatch_states(&mut conn, user_id, m.entity, &[m.entity_id])
                .await?
                .pop();
            Ok(error_result(m.mutation_id, &e, state))
        }
    }
}

async fn dispatch_apply(
    tx: &mut PgConnection,
    user_id: Uuid,
    m: &Mutation,
) -> ApiResult<(i64, bool, Option<EntityState>)> {
    match m.entity {
        Entity::Account => apply_one::<Accounts>(tx, user_id, m).await,
        Entity::Category => apply_one::<Categories>(tx, user_id, m).await,
        Entity::Transaction => apply_one::<Transactions>(tx, user_id, m).await,
        Entity::InstallmentPlan => apply_one::<InstallmentPlans>(tx, user_id, m).await,
        Entity::Budget => apply_one::<Budgets>(tx, user_id, m).await,
    }
}

/// `(version, conflict, state after)`.
async fn apply_one<R: Resource>(
    tx: &mut PgConnection,
    user_id: Uuid,
    m: &Mutation,
) -> ApiResult<(i64, bool, Option<EntityState>)> {
    let (version, conflict) = match m.op {
        MutationOp::Put => {
            let payload = m
                .payload
                .clone()
                .ok_or_else(|| ApiError::validation("payload: required for put"))?;
            let data: R::Data = serde_json::from_value(payload)
                .map_err(|e| ApiError::validation(format!("payload: {e}")))?;
            let put = Put {
                base_version: m.base_version,
                data,
            };
            let out = repo::put_in::<R>(tx, user_id, m.entity_id, put).await?;
            (out.record.version, out.conflict)
        }
        MutationOp::Delete => (repo::delete_in::<R>(tx, user_id, m.entity_id).await?, false),
    };
    let state = repo::states::<R>(tx, user_id, &[m.entity_id]).await?.pop();
    Ok((version, conflict, state))
}

async fn dispatch_states(
    conn: &mut PgConnection,
    user_id: Uuid,
    entity: Entity,
    ids: &[Uuid],
) -> ApiResult<Vec<EntityState>> {
    match entity {
        Entity::Account => repo::states::<Accounts>(conn, user_id, ids).await,
        Entity::Category => repo::states::<Categories>(conn, user_id, ids).await,
        Entity::Transaction => repo::states::<Transactions>(conn, user_id, ids).await,
        Entity::InstallmentPlan => repo::states::<InstallmentPlans>(conn, user_id, ids).await,
        Entity::Budget => repo::states::<Budgets>(conn, user_id, ids).await,
    }
}

async fn pull(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<PullQuery>,
) -> ApiResult<Json<PullResponse>> {
    let limit = q
        .limit
        .unwrap_or(DEFAULT_PULL_LIMIT)
        .clamp(1, MAX_PULL_LIMIT);
    let mut conn = state.db.acquire().await?;
    // One row more than the page, to know whether there is more.
    let mut rows: Vec<(i64, String, Uuid)> = sqlx::query_as(
        "SELECT seq, entity, entity_id FROM change_log \
         WHERE user_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
    )
    .bind(user.user_id)
    .bind(q.since)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut *conn)
    .await?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_since = rows.last().map_or(q.since, |r| r.0);

    // Latest seq of each record in the page.
    let mut latest: HashMap<(Entity, Uuid), i64> = HashMap::new();
    for (seq, entity, id) in rows {
        let entity = Entity::parse(&entity).ok_or(ApiError::Internal)?;
        latest.insert((entity, id), seq);
    }
    let mut by_entity: BTreeMap<Entity, Vec<Uuid>> = BTreeMap::new();
    for (entity, id) in latest.keys() {
        by_entity.entry(*entity).or_default().push(*id);
    }
    let mut changes = Vec::with_capacity(latest.len());
    for (entity, ids) in by_entity {
        for s in dispatch_states(&mut conn, user.user_id, entity, &ids).await? {
            if let Some(&seq) = latest.get(&(s.entity, s.id)) {
                changes.push(Change { seq, state: s });
            }
        }
    }
    changes.sort_by_key(|c| c.seq);
    Ok(Json(PullResponse {
        changes,
        next_since,
        has_more,
    }))
}
