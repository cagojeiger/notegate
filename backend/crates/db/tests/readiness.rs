//! Integration tests for DB readiness checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::TestDb;

#[tokio::test]
async fn readiness_accepts_migrated_schema() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };

    notegate_db::check_readiness(&db.pool).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn readiness_rejects_missing_migration_row() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };

    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&db.pool)
        .await?;

    let err = notegate_db::check_readiness(&db.pool)
        .await
        .expect_err("missing migration row is not ready");
    assert!(err.to_string().contains("migration 1 has not been applied"));

    db.cleanup().await;
    Ok(())
}
