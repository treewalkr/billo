//! Sync protocol (architecture §5): the client pushes its outbox of mutations with
//! `POST /api/v1/sync/push` and pulls the user's change feed with
//! `GET /api/v1/sync/pull?since=<seq>`.
//!
//! Payloads and records are JSON objects in the same shape as the CRUD resources
//! (`Put<D>` without `base_version`, and `Record<D>`), so this module stays generic.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ErrorDetail;

/// Most mutations accepted in one push.
pub const MAX_PUSH_MUTATIONS: usize = 200;
/// Default and largest page size of a pull.
pub const DEFAULT_PULL_LIMIT: u32 = 500;
pub const MAX_PULL_LIMIT: u32 = 1000;

/// The synced record types. The names match `change_log.entity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Account,
    Category,
    Transaction,
    InstallmentPlan,
    Budget,
}

impl Entity {
    pub const ALL: [Self; 5] = [
        Self::Account,
        Self::Category,
        Self::Transaction,
        Self::InstallmentPlan,
        Self::Budget,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Category => "category",
            Self::Transaction => "transaction",
            Self::InstallmentPlan => "installment_plan",
            Self::Budget => "budget",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOp {
    /// Create or update with `payload`.
    Put,
    /// Soft delete.
    Delete,
}

/// One queued change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mutation {
    /// Idempotency key: a mutation is applied at most once per user.
    pub mutation_id: Uuid,
    pub entity: Entity,
    pub entity_id: Uuid,
    /// The version the client last saw; `None` when creating.
    #[serde(default)]
    pub base_version: Option<i64>,
    pub op: MutationOp,
    /// The resource's fields for `put`; ignored for `delete`.
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// Body of `POST /api/v1/sync/push`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushRequest {
    pub mutations: Vec<Mutation>,
}

/// Body of the push response: one result per mutation, in request order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushResponse {
    pub results: Vec<MutationResult>,
}

/// The current server state of one record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityState {
    pub entity: Entity,
    pub id: Uuid,
    pub version: i64,
    /// A tombstone: the record was deleted; `record` is `None`.
    pub deleted: bool,
    /// The record (`Record<D>` JSON) when not deleted.
    #[serde(default)]
    pub record: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationResult {
    pub mutation_id: Uuid,
    /// Applied (now, or earlier with the same `mutation_id`).
    pub ok: bool,
    /// `base_version` was stale; the write was applied anyway (last write wins).
    #[serde(default)]
    pub conflict: bool,
    /// The record's version after the write.
    #[serde(default)]
    pub version: Option<i64>,
    /// Why it was rejected (`validation`, `not_found`, `conflict` for an id owned by
    /// another user, `internal` for a failure worth retrying).
    #[serde(default)]
    pub error: Option<ErrorDetail>,
    /// The record's server state after the mutation, if the user has it.
    #[serde(default)]
    pub state: Option<EntityState>,
}

/// Query of `GET /api/v1/sync/pull`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullQuery {
    /// Last `seq` the client has applied; 0 for everything.
    #[serde(default)]
    pub since: i64,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// One changed record in a pull page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// The feed position of the record's latest change in this page.
    pub seq: i64,
    #[serde(flatten)]
    pub state: EntityState,
}

/// Body of `GET /api/v1/sync/pull`: changes ordered by `seq`, each record at most once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullResponse {
    pub changes: Vec<Change>,
    /// Pass as `since` for the next page (unchanged when there were no changes).
    pub next_since: i64,
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_names_round_trip() {
        for e in Entity::ALL {
            assert_eq!(Entity::parse(e.as_str()), Some(e));
            assert_eq!(
                serde_json::to_value(e).unwrap(),
                serde_json::Value::String(e.as_str().into())
            );
        }
        assert_eq!(Entity::parse("user"), None);
    }

    #[test]
    fn mutation_defaults() {
        let m: Mutation = serde_json::from_value(serde_json::json!({
            "mutation_id": Uuid::nil(),
            "entity": "installment_plan",
            "entity_id": Uuid::nil(),
            "op": "delete",
        }))
        .unwrap();
        assert_eq!(m.entity, Entity::InstallmentPlan);
        assert_eq!(m.base_version, None);
        assert_eq!(m.payload, None);
    }

    #[test]
    fn change_is_flat() {
        let c = Change {
            seq: 7,
            state: EntityState {
                entity: Entity::Budget,
                id: Uuid::nil(),
                version: 3,
                deleted: true,
                record: None,
            },
        };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["seq"], 7);
        assert_eq!(json["entity"], "budget");
        assert_eq!(json["deleted"], true);
        assert_eq!(serde_json::from_value::<Change>(json).unwrap(), c);
    }
}
