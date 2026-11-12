//! `/api/v1/budgets`

use billo_api::{BudgetData, BudgetQuery};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{PgConnection, Postgres, QueryBuilder, Row};
use uuid::Uuid;

use super::money_from_row;
use crate::error::{ApiError, ApiResult};
use crate::repo::{Resource, owned_row};
use crate::validate;

pub struct Budgets;

impl Resource for Budgets {
    const TABLE: &'static str = "budgets";
    const ENTITY: &'static str = "budget";
    const COLUMNS: &'static [&'static str] =
        &["category_id", "period_month", "limit_minor", "currency"];
    const ORDER_BY: &'static str = "period_month DESC, id";

    type Data = BudgetData;
    type Filter = BudgetQuery;

    fn data_from_row(row: &PgRow) -> sqlx::Result<BudgetData> {
        Ok(BudgetData {
            category_id: row.try_get("category_id")?,
            period_month: row.try_get("period_month")?,
            limit: money_from_row(row, "limit_minor", "currency")?,
        })
    }

    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        d: &BudgetData,
    ) -> Query<'q, Postgres, PgArguments> {
        q.bind(d.category_id)
            .bind(d.period_month)
            .bind(d.limit.minor)
            .bind(d.limit.currency.clone())
    }

    async fn validate(
        conn: &mut PgConnection,
        user_id: Uuid,
        d: BudgetData,
    ) -> ApiResult<BudgetData> {
        let limit = validate::money("limit", &d.limit, false)?;
        if limit.is_negative() {
            return Err(ApiError::validation("limit: must not be negative"));
        }
        if d.period_month.day() != 1 {
            return Err(ApiError::validation(
                "period_month: must be the first day of a month",
            ));
        }
        if owned_row(conn, "categories", "id", user_id, d.category_id)
            .await?
            .is_none()
        {
            return Err(ApiError::validation("category_id: no such category"));
        }
        Ok(d)
    }

    fn filter(f: &BudgetQuery, qb: &mut QueryBuilder<Postgres>) {
        if let Some(month) = f.period_month {
            qb.push(" AND period_month = ").push_bind(month);
        }
    }
}
