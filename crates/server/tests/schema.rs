//! Schema checks against a real Postgres. Skipped when `DATABASE_URL` is unset.

use sqlx::postgres::PgPoolOptions;

fn database_url() -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty());
    if url.is_none() {
        eprintln!("skipping: DATABASE_URL is not set");
    }
    url
}

#[tokio::test]
async fn core_tables_exist() {
    let Some(url) = database_url() else { return };
    let db = PgPoolOptions::new().connect(&url).await.unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();
    for table in [
        "sessions",
        "accounts",
        "categories",
        "transactions",
        "installment_plans",
        "budgets",
        "change_log",
    ] {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = $1",
        )
        .bind(table)
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(n, 1, "missing table {table}");
    }
}

#[tokio::test]
async fn no_floating_or_numeric_columns() {
    let Some(url) = database_url() else { return };
    let db = PgPoolOptions::new().connect(&url).await.unwrap();
    billo_server::MIGRATOR.run(&db).await.unwrap();
    let cols: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' \
           AND data_type IN ('real', 'double precision', 'numeric', 'money')",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert!(cols.is_empty(), "inexact money types: {cols:?}");
}
