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
use notegate_core::Error;
use notegate_db::{ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::CreateApiKey;
use uuid::Uuid;

/// Insert one live key with a unique token hash via the capped insert path.
async fn insert_capped(
    repo: &ApiKeyRepo,
    account_id: Uuid,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    repo.insert_key_with_cap(InsertApiKey {
        key_id: Uuid::new_v4(),
        account_id,
        command: &CreateApiKey {
            name: label.to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        },
        token_prefix: "ngk_v1_test",
        token_hash: &format!("hash-{label}-{}", Uuid::new_v4()),
        created_by: account_id,
        rotated_from_key_id: None,
    })
    .await?;
    Ok(())
}

async fn insert_agent_account(
    pool: &sqlx::PgPool,
    creator: Uuid,
    name: &str,
) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
        .fetch_one(pool)
        .await?;
    sqlx::query("INSERT INTO agents (id, name, created_by) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(creator)
        .execute(pool)
        .await?;
    Ok(id)
}

/// P0-1: concurrent capped creates against one account never exceed the cap.
/// Runs against real Postgres so the in-tx `FOR UPDATE` serialization is exercised.
async fn concurrent_create_respects_cap(
    pool: &sqlx::PgPool,
    account_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = ApiKeyRepo::new(pool.clone());
    // Seed the account at cap-1 live keys.
    let max = notegate_core::limits::API_KEYS_PER_ACCOUNT_MAX;
    for index in 0..(max - 1) {
        insert_capped(&repo, account_id, &format!("seed-{index}")).await?;
    }
    assert_eq!(repo.count_live_keys(account_id).await?, max - 1);

    // Spawn N concurrent creates for the single remaining slot.
    let mut handles = Vec::new();
    for index in 0..8 {
        let repo = repo.clone();
        handles.push(tokio::spawn(async move {
            repo.insert_key_with_cap(InsertApiKey {
                key_id: Uuid::new_v4(),
                account_id,
                command: &CreateApiKey {
                    name: format!("race-{index}"),
                    scopes: Vec::new(),
                    expires_at: None,
                },
                token_prefix: "ngk_v1_test",
                token_hash: &format!("hash-race-{index}-{}", Uuid::new_v4()),
                created_by: account_id,
                rotated_from_key_id: None,
            })
            .await
        }));
    }

    let mut wins = 0;
    let mut conflicts = 0;
    for handle in handles {
        match handle.await? {
            Ok(_) => wins += 1,
            Err(Error::Conflict(_)) => conflicts += 1,
            Err(other) => return Err(other.into()),
        }
    }

    assert_eq!(wins, 1, "exactly one over-cap create must win");
    assert_eq!(conflicts, 7, "the rest must get Conflict");
    let live = repo.count_live_keys(account_id).await?;
    assert!(live <= max, "live keys {live} must not exceed cap {max}");
    assert_eq!(live, max, "the account must end exactly at the cap");
    Ok(())
}

#[tokio::test]
async fn concurrent_create_respects_cap_for_user_account()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user_id = insert_user_account(&db.pool, "race-user", "race-user@example.test").await?;
    concurrent_create_respects_cap(&db.pool, user_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_create_respects_cap_for_agent_account()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let creator = insert_user_account(&db.pool, "race-owner", "race-owner@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, creator, "race-agent").await?;
    concurrent_create_respects_cap(&db.pool, agent_id).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_account_excludes_revoked_keys() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "list-user", "list-user@example.test").await?;

    insert_capped(&repo, user_id, "live-a").await?;
    insert_capped(&repo, user_id, "to-revoke").await?;
    insert_capped(&repo, user_id, "live-b").await?;

    let before = repo.list_by_account(user_id).await?;
    assert_eq!(before.len(), 3);
    let revoke_id = before
        .iter()
        .find(|k| k.name == "to-revoke")
        .map(|k| k.id)
        .expect("revoke target present");

    repo.revoke_key(user_id, revoke_id, user_id, Some("test"))
        .await?;

    let after = repo.list_by_account(user_id).await?;
    assert_eq!(after.len(), 2, "revoked key must be excluded");
    assert!(
        after.iter().all(|k| k.name != "to-revoke"),
        "revoked key must not appear"
    );
    // Ordering unchanged: created_at DESC, id DESC -> newest ("live-b") first.
    assert_eq!(after.first().map(|k| k.name.as_str()), Some("live-b"));

    db.cleanup().await;
    Ok(())
}

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
