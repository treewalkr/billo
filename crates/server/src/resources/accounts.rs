//! `/api/v1/accounts`

use billo_api::{AccountData, AccountRules};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::types::Json;
use sqlx::{PgConnection, Postgres, Row};
use uuid::Uuid;

use super::{enum_from_db, enum_to_db};
use crate::error::{ApiError, ApiResult};
use crate::repo::{NoFilter, Resource};
use crate::validate;

pub struct Accounts;

impl Resource for Accounts {
    const TABLE: &'static str = "accounts";
    const ENTITY_KIND: billo_api::sync::Entity = billo_api::sync::Entity::Account;
    const COLUMNS: &'static [&'static str] = &[
        "kind",
        "name",
        "currency",
        "rules",
        "opened_on",
        "closed_on",
    ];

    type Data = AccountData;
    type Filter = NoFilter;

    fn data_from_row(row: &PgRow) -> sqlx::Result<AccountData> {
        let Json(rules): Json<AccountRules> = row.try_get("rules")?;
        Ok(AccountData {
            kind: enum_from_db(row.try_get("kind")?)?,
            name: row.try_get("name")?,
            currency: row.try_get("currency")?,
            rules,
            opened_on: row.try_get("opened_on")?,
            closed_on: row.try_get("closed_on")?,
        })
    }

    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        d: &AccountData,
    ) -> Query<'q, Postgres, PgArguments> {
        q.bind(enum_to_db(&d.kind))
            .bind(d.name.clone())
            .bind(d.currency.clone())
            .bind(Json(d.rules.clone()))
            .bind(d.opened_on)
            .bind(d.closed_on)
    }

    async fn validate(
        _conn: &mut PgConnection,
        _user_id: Uuid,
        mut d: AccountData,
    ) -> ApiResult<AccountData> {
        d.name = validate::name("name", &d.name)?;
        let currency = validate::currency("currency", &d.currency)?;
        validate::account_rules(d.kind, currency, &d.rules)?;
        if let (Some(open), Some(close)) = (d.opened_on, d.closed_on)
            && close < open
        {
            return Err(ApiError::validation("closed_on: before opened_on"));
        }
        Ok(d)
    }
}
