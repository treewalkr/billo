//! `/api/v1/installment-plans`

use billo_api::rules::RoundingSettings;
use billo_api::{AccountKind, InstallmentPlanData, Remainder};
use billo_domain::card::InstallmentPlan;
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{PgConnection, Postgres, Row};
use uuid::Uuid;

use super::{enum_from_db, enum_to_db, money_from_row};
use crate::error::{ApiError, ApiResult};
use crate::repo::{NoFilter, Resource, owned_row};
use crate::validate;

pub struct InstallmentPlans;

fn decode_err(e: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(e))
}

impl Resource for InstallmentPlans {
    const TABLE: &'static str = "installment_plans";
    const ENTITY_KIND: billo_api::sync::Entity = billo_api::sync::Entity::InstallmentPlan;
    const COLUMNS: &'static [&'static str] = &[
        "card_account_id",
        "principal_minor",
        "currency",
        "months",
        "monthly_rate_bp",
        "first_post_on",
        "closed_on",
        "rounding_step",
        "rounding_mode",
        "remainder",
    ];
    const ORDER_BY: &'static str = "first_post_on DESC, id";

    type Data = InstallmentPlanData;
    type Filter = NoFilter;

    fn data_from_row(row: &PgRow) -> sqlx::Result<InstallmentPlanData> {
        let months: i32 = row.try_get("months")?;
        let rate: i32 = row.try_get("monthly_rate_bp")?;
        Ok(InstallmentPlanData {
            card_account_id: row.try_get("card_account_id")?,
            principal: money_from_row(row, "principal_minor", "currency")?,
            months: u16::try_from(months).map_err(decode_err)?,
            monthly_rate_bp: u32::try_from(rate).map_err(decode_err)?,
            first_post_on: row.try_get("first_post_on")?,
            closed_on: row.try_get("closed_on")?,
            rounding: RoundingSettings {
                step: row.try_get("rounding_step")?,
                mode: enum_from_db(row.try_get("rounding_mode")?)?,
            },
            remainder: enum_from_db(row.try_get("remainder")?)?,
        })
    }

    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        d: &InstallmentPlanData,
    ) -> Query<'q, Postgres, PgArguments> {
        // `validate` caps the rate at 10_000, so it fits; an out-of-range value would
        // fail the column's CHECK rather than wrap.
        let rate = i32::try_from(d.monthly_rate_bp).unwrap_or(i32::MAX);
        q.bind(d.card_account_id)
            .bind(d.principal.minor)
            .bind(d.principal.currency.clone())
            .bind(i32::from(d.months))
            .bind(rate)
            .bind(d.first_post_on)
            .bind(d.closed_on)
            .bind(d.rounding.step)
            .bind(enum_to_db(&d.rounding.mode))
            .bind(enum_to_db(&d.remainder))
    }

    async fn validate(
        conn: &mut PgConnection,
        user_id: Uuid,
        d: InstallmentPlanData,
    ) -> ApiResult<InstallmentPlanData> {
        let principal = validate::money("principal", &d.principal, true)?;
        let remainder = match d.remainder {
            Remainder::First => billo_domain::card::Remainder::First,
            Remainder::Last => billo_domain::card::Remainder::Last,
        };
        InstallmentPlan::new(
            principal,
            d.months,
            d.monthly_rate_bp,
            d.first_post_on,
            validate::rounding("rounding", d.rounding)?,
            remainder,
        )
        .map_err(|e| ApiError::validation(format!("installment plan: {e}")))?;
        if d.closed_on.is_some_and(|c| c < d.first_post_on) {
            return Err(ApiError::validation("closed_on: before first_post_on"));
        }

        let card = owned_row(
            conn,
            "accounts",
            "kind, currency",
            user_id,
            d.card_account_id,
        )
        .await?
        .ok_or_else(|| ApiError::validation("card_account_id: no such account"))?;
        let kind: AccountKind = enum_from_db(card.try_get("kind")?)?;
        if !matches!(kind, AccountKind::Card | AccountKind::Revolving) {
            return Err(ApiError::validation(
                "card_account_id: not a card or revolving account",
            ));
        }
        let currency: String = card.try_get("currency")?;
        if currency != d.principal.currency {
            return Err(ApiError::validation(
                "principal.currency: must be the card's currency",
            ));
        }
        Ok(d)
    }
}
