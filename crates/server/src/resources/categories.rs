//! `/api/v1/categories`

use billo_api::CategoryData;
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{PgConnection, Postgres, Row};
use uuid::Uuid;

use super::{enum_from_db, enum_to_db};
use crate::error::{ApiError, ApiResult};
use crate::repo::{NoFilter, Resource};
use crate::validate;

pub struct Categories;

impl Resource for Categories {
    const TABLE: &'static str = "categories";
    const ENTITY_KIND: billo_api::sync::Entity = billo_api::sync::Entity::Category;
    const COLUMNS: &'static [&'static str] = &["parent_id", "name_th", "name_en", "kind"];

    type Data = CategoryData;
    type Filter = NoFilter;

    fn data_from_row(row: &PgRow) -> sqlx::Result<CategoryData> {
        Ok(CategoryData {
            parent_id: row.try_get("parent_id")?,
            name_th: row.try_get("name_th")?,
            name_en: row.try_get("name_en")?,
            kind: enum_from_db(row.try_get("kind")?)?,
        })
    }

    fn bind<'q>(
        q: Query<'q, Postgres, PgArguments>,
        d: &CategoryData,
    ) -> Query<'q, Postgres, PgArguments> {
        q.bind(d.parent_id)
            .bind(d.name_th.clone())
            .bind(d.name_en.clone())
            .bind(enum_to_db(&d.kind))
    }

    async fn validate(
        conn: &mut PgConnection,
        user_id: Uuid,
        mut d: CategoryData,
    ) -> ApiResult<CategoryData> {
        d.name_th = validate::optional_name("name_th", d.name_th.as_deref())?;
        d.name_en = validate::optional_name("name_en", d.name_en.as_deref())?;
        if d.name_th.is_none() && d.name_en.is_none() {
            return Err(ApiError::validation("name_th or name_en: required"));
        }
        if let Some(parent) = d.parent_id
            && crate::repo::owned_row(conn, "categories", "id", user_id, parent)
                .await?
                .is_none()
        {
            return Err(ApiError::validation("parent_id: no such category"));
        }
        Ok(d)
    }
}
