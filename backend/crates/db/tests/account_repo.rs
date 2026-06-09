//! Integration tests for `AccountRepo` against a real Postgres schema.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test account_repo`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::TestDb;
use notegate_db::AccountRepo;
use notegate_model::ResolveAttrs;
use sqlx::Row as _;

fn attrs(sub: &str, email: &str, name: &str) -> ResolveAttrs {
    ResolveAttrs {
        sub: sub.to_owned(),
        email: email.to_owned(),
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn upsert_user_creates_account_and_user_rows() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, user) = repo
        .upsert_user_by_sub(&attrs("sub-1", "a@example.test", "Kang"))
        .await?;
    assert_eq!(account.id, user.id);
    assert_eq!(account.kind.as_str(), "user");
    assert_eq!(account.display_name, "Kang");
    assert!(account.is_active);
    assert_eq!(user.email.as_deref(), Some("a@example.test"));

    let plaintext_matches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM accounts \
         WHERE display_name_ciphertext::text LIKE '%Kang%'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(plaintext_matches, 0);

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(accounts, 1);
    assert_eq!(users, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn duplicate_sub_updates_and_does_not_duplicate() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, _) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "old@example.test", "Old Name"))
        .await?;
    let (second, user) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "new@example.test", "New Name"))
        .await?;

    assert_eq!(first.id, second.id, "same sub must reuse the same account");
    assert_eq!(second.display_name, "New Name");
    assert_eq!(user.email.as_deref(), Some("new@example.test"));

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        accounts, 1,
        "duplicate sub must not create a second account"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn find_user_by_sub_and_account_resolve_the_pair() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (created, _) = repo
        .upsert_user_by_sub(&attrs("sub-find", "f@example.test", "Finder"))
        .await?;

    let (account, user) = repo
        .find_user_by_sub("sub-find")
        .await?
        .ok_or("user resolves by sub")?;
    assert_eq!(account.id, created.id);
    assert_eq!(user.email.as_deref(), Some("f@example.test"));

    let by_id = repo.find_caller_by_account_id(created.id).await?;
    assert_eq!(by_id.map(|(a, _)| a.id), Some(created.id));

    let by_account = repo.find_account(created.id).await?;
    assert_eq!(by_account.map(|a| a.id), Some(created.id));

    assert!(repo.find_user_by_sub("nope").await?.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn anonymize_user_clears_pii_and_deactivates() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, _) = repo
        .upsert_user_by_sub(&attrs("sub-gone", "g@example.test", "Gone"))
        .await?;

    repo.anonymize_user(account.id, account.id).await?;

    let after = repo
        .find_account(account.id)
        .await?
        .ok_or("account still present after soft delete")?;
    assert!(!after.is_active);
    assert!(after.deleted_at.is_some());
    assert_eq!(after.deleted_by, Some(account.id));

    let row = sqlx::query(
        "SELECT provider_sub_hash, email_hash, email_ciphertext, anonymized_at \
         FROM users WHERE id = $1",
    )
    .bind(account.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(row.get::<Option<String>, _>("provider_sub_hash").is_none());
    assert!(row.get::<Option<String>, _>("email_hash").is_none());
    assert!(row.get::<Option<Vec<u8>>, _>("email_ciphertext").is_none());
    assert!(
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("anonymized_at")
            .is_some()
    );

    db.cleanup().await;
    Ok(())
}
