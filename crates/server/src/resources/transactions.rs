//! `/api/v1/transactions`

use billo_api::{TransactionData, TransactionKind, TransactionQuery};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{PgConnection, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use super::{enum_from_db, enum_to_db, money_from_row};
use crate::error::{ApiError, ApiResult};
use crate::repo::{Resource, owned_row};
use crate::validate;

pub struct Transactions;

impl Resource for Transactions {
    const TABLE: &'static str = "transactions";
    const ENTITY_KIND: billo_api::sync::Entity = billo_api::sync::Entity::Transaction;
    const COLUMNS: &'static [&'static str] = &[
        "account_id",
        "category_id",
        "occurred_on",
        "posted_on",
        "amount_minor",
        "currency",
        "orig_amount_minor",
        "orig_currency",
        "kind",
        "transfer_peer_id",
        "installment_plan_id",
        "note",
    ];
    const ORDER_BY: &'static str = "occurred_on DESC, id DESC";

    type Data = TransactionData;
    type Filter = TransactionQuery;

    fn data_from_row(row: &PgRow) -> sqlx::Result<TransactionData> {
        let orig_minor: Option<i64> = row.try_get("orig_amount_minor")?;
        let orig_currency: Option<String> = row.try_get("orig_currency")?;
        Ok(TransactionData {
            account_id: row.try_get("account_id")?,
            category_id: row.try_get("category_id")?,
            occurred_on: row.try_get("occurred_on")?,
            posted_on: row.try_get("posted_on")?,
            amount: money_from_row(row, "amount_minor", "currency")?,
            orig_amount: orig_minor
                .zip(orig_currency)
                .map(|(minor, currency)| billo_api::Money { minor, currency }),
            kind: enum_from_db(row.try_get("kind")?)?,
            transfer_peer_id: row.try_get("transfer_peer_id")?,
            installment_plan_id: row.try_get("installment_plan_id")?,
            note: row.try_get("note")?,
        })
    }

    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        d: &TransactionData,
    ) -> Query<'q, Postgres, PgArguments> {
        q.bind(d.account_id)
            .bind(d.category_id)
            .bind(d.occurred_on)
            .bind(d.posted_on)
            .bind(d.amount.minor)
            .bind(d.amount.currency.clone())
            .bind(d.orig_amount.as_ref().map(|m| m.minor))
            .bind(d.orig_amount.as_ref().map(|m| m.currency.clone()))
            .bind(enum_to_db(&d.kind))
            .bind(d.transfer_peer_id)
            .bind(d.installment_plan_id)
            .bind(d.note.clone())
    }

    async fn validate(
        conn: &mut PgConnection,
        user_id: Uuid,
        mut d: TransactionData,
    ) -> ApiResult<TransactionData> {
        let amount = validate::money("amount", &d.amount, false)?;
        match d.kind {
            TransactionKind::InterestAdjust if amount.is_zero() => {
                return Err(ApiError::validation("amount: must not be zero"));
            }
            TransactionKind::InterestAdjust => {}
            _ if amount.minor() <= 0 => {
                return Err(ApiError::validation("amount: must be positive"));
            }
            _ => {}
        }
        if let Some(orig) = &d.orig_amount {
            validate::money("orig_amount", orig, true)?;
        }
        if d.transfer_peer_id.is_some() && d.kind != TransactionKind::Transfer {
            return Err(ApiError::validation("transfer_peer_id: only for transfers"));
        }
        d.note = match d.note.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(n) if n.chars().count() > validate::MAX_NOTE_CHARS => {
                return Err(ApiError::validation("note: too long"));
            }
            Some(n) => Some(n.to_owned()),
        };

        let account = owned_row(conn, "accounts", "currency", user_id, d.account_id)
            .await?
            .ok_or_else(|| ApiError::validation("account_id: no such account"))?;
        let account_currency: String = account.try_get("currency")?;
        if account_currency != d.amount.currency {
            return Err(ApiError::validation(
                "amount.currency: must be the account's currency",
            ));
        }
        if let Some(category) = d.category_id
            && owned_row(conn, "categories", "id", user_id, category)
                .await?
                .is_none()
        {
            return Err(ApiError::validation("category_id: no such category"));
        }
        if let Some(plan) = d.installment_plan_id {
            let plan = owned_row(conn, "installment_plans", "card_account_id", user_id, plan)
                .await?
                .ok_or_else(|| ApiError::validation("installment_plan_id: no such plan"))?;
            let card: Uuid = plan.try_get("card_account_id")?;
            if card != d.account_id {
                return Err(ApiError::validation(
                    "installment_plan_id: plan belongs to another account",
                ));
            }
        }
        Ok(d)
    }

    fn filter(f: &TransactionQuery, qb: &mut QueryBuilder<Postgres>) {
        if let Some(account) = f.account_id {
            qb.push(" AND account_id = ").push_bind(account);
        }
        if let Some(from) = f.from {
            qb.push(" AND occurred_on >= ").push_bind(from);
        }
        if let Some(to) = f.to {
            qb.push(" AND occurred_on <= ").push_bind(to);
        }
    }
}
