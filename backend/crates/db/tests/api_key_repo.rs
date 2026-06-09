//! Integration tests for unified `ApiKeyRepo` lookup.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::{ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::CreateApiKey;
use uuid::Uuid;

#[tokio::test]
async fn live_user_api_key_resolves_account_and_marks_last_used()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "api-key-user", "user@example.test").await?;
    let key_id = Uuid::new_v4();

    repo.insert_key(InsertApiKey {
        key_id,
        account_id: user_id,
        command: &CreateApiKey {
            name: "user-key".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        },
        token_prefix: "ngk_v1_test",
        token_hash: "hash-user-key",
        created_by: user_id,
        rotated_from_key_id: None,
    })
    .await?;

    let resolved = repo
        .find_live_account_id_by_token_hash("hash-user-key")
        .await?;
    assert_eq!(resolved, Some(user_id));

    let last_used_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(last_used_at.is_some());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rotate_key_is_atomic_and_excludes_old_key_from_live_cap()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "rotate-user", "rotate@example.test").await?;
    let first_key_id = Uuid::new_v4();

    for index in 0..notegate_core::limits::API_KEYS_PER_ACCOUNT_MAX {
        let key_id = if index == 0 {
            first_key_id
        } else {
            Uuid::new_v4()
        };
        let token_hash = format!("hash-{index}");
        repo.insert_key(InsertApiKey {
            key_id,
            account_id: user_id,
            command: &CreateApiKey {
                name: format!("key-{index}"),
                scopes: Vec::new(),
                expires_at: None,
            },
            token_prefix: "ngk_v1_test",
            token_hash: &token_hash,
            created_by: user_id,
            rotated_from_key_id: None,
        })
        .await?;
    }
    assert_eq!(
        repo.count_live_keys(user_id).await?,
        notegate_core::limits::API_KEYS_PER_ACCOUNT_MAX
    );

    let new_key_id = Uuid::new_v4();
    let rotated = repo
        .rotate_key(
            InsertApiKey {
                key_id: new_key_id,
                account_id: user_id,
                command: &CreateApiKey {
                    name: "key-0".to_owned(),
                    scopes: Vec::new(),
                    expires_at: None,
                },
                token_prefix: "ngk_v1_rotated",
                token_hash: "hash-rotated",
                created_by: user_id,
                rotated_from_key_id: Some(first_key_id),
            },
            first_key_id,
            user_id,
        )
        .await?;

    assert_eq!(rotated.rotated_from_key_id, Some(first_key_id));
    assert_eq!(
        repo.count_live_keys(user_id).await?,
        notegate_core::limits::API_KEYS_PER_ACCOUNT_MAX
    );

    let old: (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT revoked_at, revoked_reason FROM api_keys WHERE id = $1")
            .bind(first_key_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(old.0.is_some());
    assert_eq!(old.1.as_deref(), Some("rotated"));

    db.cleanup().await;
    Ok(())
}
