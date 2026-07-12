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

#[tokio::test]
async fn readiness_rejects_missing_space_usage_table() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query("DROP TABLE space_usage CASCADE")
        .execute(&db.pool)
        .await?;

    let err = notegate_db::check_readiness(&db.pool)
        .await
        .expect_err("missing usage table is not ready");
    assert!(
        err.to_string()
            .contains("required space usage schema is not installed")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn readiness_rejects_missing_space_usage_trigger() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query("DROP TRIGGER spaces_create_usage ON spaces")
        .execute(&db.pool)
        .await?;

    let err = notegate_db::check_readiness(&db.pool)
        .await
        .expect_err("missing usage trigger is not ready");
    assert!(
        err.to_string()
            .contains("required space usage schema is not installed")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn readiness_rejects_disabled_space_usage_trigger() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query("ALTER TABLE spaces DISABLE TRIGGER spaces_create_usage")
        .execute(&db.pool)
        .await?;

    let err = notegate_db::check_readiness(&db.pool)
        .await
        .expect_err("disabled usage trigger is not ready");
    assert!(
        err.to_string()
            .contains("required space usage schema is not installed")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn crypto_key_epoch_ensure_and_verify() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query("DELETE FROM crypto_key_epochs")
        .execute(&db.pool)
        .await?;

    let crypto = notegate_core::security::PiiCrypto::test();
    let repo = notegate_db::CryptoKeyEpochRepo::new(db.pool.clone());

    assert!(repo.verify_active(&crypto).await.is_err());
    repo.ensure_active(&crypto).await?;
    repo.verify_active(&crypto).await?;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM crypto_key_epochs")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(count, 2);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn ensure_rejects_different_active_key() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    sqlx::query("DELETE FROM crypto_key_epochs")
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO crypto_key_epochs \
         (key_id, domain, status, verify_tag, version, activated_at) \
         VALUES ('other-enc', 'enc', 'active', 'other-tag', 1, now())",
    )
    .execute(&db.pool)
    .await?;

    let crypto = notegate_core::security::PiiCrypto::test();
    let repo = notegate_db::CryptoKeyEpochRepo::new(db.pool.clone());

    let err = repo
        .ensure_active(&crypto)
        .await
        .expect_err("different active key must fail startup ensure");
    assert!(err.to_string().contains("active enc crypto key epoch"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn crypto_key_epoch_verify_rejects_wrong_secret() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let wrong_crypto = notegate_core::security::PiiCrypto::from_root_secrets(
        "test-enc",
        &secrecy::SecretString::from("wrong-enc-root-secret-32-bytes-long".to_owned()),
        "test-lookup",
        &secrecy::SecretString::from("wrong-lookup-root-secret-32-bytes-long".to_owned()),
    )?;
    let repo = notegate_db::CryptoKeyEpochRepo::new(db.pool.clone());

    assert!(repo.verify_active(&wrong_crypto).await.is_err());

    db.cleanup().await;
    Ok(())
}
