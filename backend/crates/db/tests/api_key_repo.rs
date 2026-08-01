//! Integration tests for unified `ApiKeyRepo` lookup.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, agent_api_key_prefix, deactivate_account, insert_user_account};
use notegate_core::{Error, limits};
use notegate_db::{ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::CreateApiKey;
use uuid::Uuid;

const TEST_API_KEYS_PER_ACCOUNT_MAX: usize = 2;

#[derive(sqlx::FromRow)]
struct AuditEventRow {
    owner_user_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    source: String,
    op_type: String,
    resource_type: String,
    resource_id: Option<Uuid>,
    reason: String,
}

/// Insert one live key with a unique token hash via the capped insert path.
async fn insert_capped(
    repo: &ApiKeyRepo,
    account_id: Uuid,
    created_by: Uuid,
    label: &str,
    max_live_keys: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let key_id = Uuid::new_v4();
    repo.insert_key_with_cap(
        InsertApiKey {
            key_id,
            account_id,
            command: &CreateApiKey {
                name: label.to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &agent_api_key_prefix(key_id),
            token_hash: &format!("hash-{label}-{}", Uuid::new_v4()),
            created_by,
            rotated_from_key_id: None,
        },
        max_live_keys,
    )
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
    sqlx::query("INSERT INTO agents (id, name, owner_user_id) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(creator)
        .execute(pool)
        .await?;
    Ok(id)
}

/// Runs against real Postgres so the in-tx `FOR UPDATE` serialization is exercised.
async fn concurrent_create_respects_cap(
    pool: &sqlx::PgPool,
    account_id: Uuid,
    created_by: Uuid,
    max: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = ApiKeyRepo::new(pool.clone());
    // Seed the account at cap-1 live keys.
    for index in 0..(max - 1) {
        insert_capped(&repo, account_id, created_by, &format!("seed-{index}"), max).await?;
    }
    assert_eq!(repo.count_live_keys(account_id).await?, max - 1);

    // Spawn N concurrent creates for the single remaining slot.
    let mut handles = Vec::new();
    for index in 0..8 {
        let repo = repo.clone();
        handles.push(tokio::spawn(async move {
            let key_id = Uuid::new_v4();
            repo.insert_key_with_cap(
                InsertApiKey {
                    key_id,
                    account_id,
                    command: &CreateApiKey {
                        name: format!("race-{index}"),
                        scopes: Vec::new(),
                        expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                    },
                    token_prefix: &agent_api_key_prefix(key_id),
                    token_hash: &format!("hash-race-{index}-{}", Uuid::new_v4()),
                    created_by,
                    rotated_from_key_id: None,
                },
                max,
            )
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
async fn insert_key_with_cap_rejects_user_account() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "race-user", "race-user@example.test").await?;
    let key_id = Uuid::new_v4();
    let err = repo
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id: user_id,
                command: &CreateApiKey {
                    name: "user-key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                },
                token_prefix: &agent_api_key_prefix(key_id),
                token_hash: "hash-user-key-rejected",
                created_by: user_id,
                rotated_from_key_id: None,
            },
            TEST_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(message) if message == "account not found"));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_create_respects_cap_for_agent_account() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let creator = insert_user_account(&db.pool, "race-owner", "race-owner@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, creator, "race-agent").await?;
    concurrent_create_respects_cap(
        &db.pool,
        agent_id,
        creator,
        notegate_core::limits::AGENT_API_KEYS_PER_ACCOUNT_MAX,
    )
    .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn insert_key_rejects_blank_or_overlong_name() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "key-user", "key-user@example.test").await?;

    for name in [
        "   ".to_owned(),
        "k".repeat(limits::API_KEY_NAME_MAX_CHARS + 1),
    ] {
        let command = CreateApiKey {
            name,
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        };
        let token_hash = format!("hash-invalid-{}", Uuid::new_v4());
        let key_id = Uuid::new_v4();
        let err = repo
            .insert_key_with_cap(
                InsertApiKey {
                    key_id,
                    account_id: user_id,
                    command: &command,
                    token_prefix: &agent_api_key_prefix(key_id),
                    token_hash: &token_hash,
                    created_by: user_id,
                    rotated_from_key_id: None,
                },
                TEST_API_KEYS_PER_ACCOUNT_MAX,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn insert_key_with_cap_rejects_inactive_account() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id =
        insert_user_account(&db.pool, "inactive-key-user", "inactive-key@example.test").await?;
    deactivate_account(&db.pool, user_id, user_id).await?;

    let command = CreateApiKey {
        name: "inactive-key".to_owned(),
        scopes: Vec::new(),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
    };
    let key_id = Uuid::new_v4();
    let err = repo
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id: user_id,
                command: &command,
                token_prefix: &agent_api_key_prefix(key_id),
                token_hash: "hash-inactive-key",
                created_by: user_id,
                rotated_from_key_id: None,
            },
            TEST_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(message) if message == "account not found"));
    assert_eq!(repo.count_live_keys(user_id).await?, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_account_returns_live_keys_only_and_pages() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "list-user", "list-user@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, owner, "list-agent").await?;

    // Seed three live keys plus one revoked and one expired key. The list must
    // surface only the live keys; dead keys are excluded (they are purged later).
    for name in ["live-a", "live-b", "live-c", "to-revoke", "to-expire"] {
        let key_id = Uuid::new_v4();
        repo.insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id: agent_id,
            command: &CreateApiKey {
                name: name.to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &agent_api_key_prefix(key_id),
            token_hash: &format!("hash-{name}"),
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await?;
    }

    let seeded = repo.list_by_account(agent_id, 10, None).await?;
    let revoke_id = seeded
        .iter()
        .find(|k| k.name == "to-revoke")
        .map(|k| k.id)
        .expect("revoke target present");
    repo.revoke_key(agent_id, revoke_id, owner, Some("test"))
        .await?;
    sqlx::query("UPDATE api_keys SET expires_at = now() - interval '1 hour' WHERE name = 'to-expire' AND account_id = $1")
        .bind(agent_id)
        .execute(&db.pool)
        .await?;

    let live = repo.list_by_account(agent_id, 10, None).await?;
    assert_eq!(live.len(), 3, "only the three live keys are listed");
    assert!(
        live.iter()
            .all(|k| k.revoked_at.is_none() && k.name != "to-expire"),
        "revoked and expired keys must be excluded from the list"
    );

    let first_page = repo.list_by_account(agent_id, 2, None).await?;
    assert_eq!(first_page.len(), 2);
    let cursor = notegate_model::ApiKeyCursor {
        created_at: first_page.last().expect("second item").created_at,
        id: first_page.last().expect("second item").id,
    };
    let second_page = repo.list_by_account(agent_id, 2, Some(&cursor)).await?;
    assert_eq!(second_page.len(), 1);
    assert!(
        !first_page.iter().any(|key| key.id == second_page[0].id),
        "keyset page must not duplicate rows"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn historical_user_owned_api_key_does_not_authenticate_or_mark_last_used()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "api-key-user", "user@example.test").await?;
    let key_id = Uuid::new_v4();
    let token_prefix = agent_api_key_prefix(key_id);

    sqlx::query("ALTER TABLE api_keys DISABLE TRIGGER api_keys_v2_agent_owner")
        .execute(&db.pool)
        .await?;
    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id,
        account_id: user_id,
        command: &CreateApiKey {
            name: "user-key".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &token_prefix,
        token_hash: "hash-user-key",
        created_by: user_id,
        rotated_from_key_id: None,
    })
    .await?;
    sqlx::query("ALTER TABLE api_keys ENABLE TRIGGER api_keys_v2_agent_owner")
        .execute(&db.pool)
        .await?;

    let resolved = repo
        .find_live_agent_id_by_key(key_id, &token_prefix, "hash-user-key")
        .await?;
    assert_eq!(resolved, None);

    let last_used_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(last_used_at.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn v2_agent_api_key_cutover_retires_existing_keys_and_blocks_invalid_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    sqlx::query("DROP TRIGGER api_keys_v2_agent_owner ON api_keys")
        .execute(&db.pool)
        .await?;
    sqlx::query("DROP FUNCTION enforce_v2_agent_api_key()")
        .execute(&db.pool)
        .await?;
    sqlx::raw_sql(include_str!(
        "../migrations/0026_enforce_agent_api_keys.sql"
    ))
    .execute(&db.pool)
    .await?;
    sqlx::query("DROP TRIGGER api_keys_agent_owner ON api_keys")
        .execute(&db.pool)
        .await?;
    let owner = insert_user_account(
        &db.pool,
        "legacy-key-owner",
        "legacy-key-owner@example.test",
    )
    .await?;
    let agent_id = insert_agent_account(&db.pool, owner, "legacy-key-agent").await?;
    let user_v1_id = Uuid::new_v4();
    let user_v2_id = Uuid::new_v4();
    let live_v1_id = Uuid::new_v4();
    let live_v2_id = Uuid::new_v4();
    let malformed_v2_id = Uuid::new_v4();
    let revoked_v1_id = Uuid::new_v4();

    for (key_id, account_id, name, prefix) in [
        (user_v1_id, owner, "user-v1", format!("ngk_v1_{user_v1_id}")),
        (
            user_v2_id,
            owner,
            "user-v2",
            agent_api_key_prefix(user_v2_id),
        ),
        (
            live_v1_id,
            agent_id,
            "live-v1",
            format!("ngk_v1_{live_v1_id}"),
        ),
        (
            live_v2_id,
            agent_id,
            "live-v2",
            agent_api_key_prefix(live_v2_id),
        ),
        (
            malformed_v2_id,
            agent_id,
            "malformed-v2",
            "ngk_v2_malformed".to_owned(),
        ),
        (
            revoked_v1_id,
            agent_id,
            "revoked-v1",
            format!("ngk_v1_{revoked_v1_id}"),
        ),
    ] {
        repo.insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id,
            command: &CreateApiKey {
                name: name.to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &prefix,
            token_hash: &format!("hash-{name}"),
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await?;
    }
    repo.revoke_key(agent_id, revoked_v1_id, owner, Some("manual-revocation"))
        .await?;

    sqlx::raw_sql(include_str!(
        "../migrations/0027_enforce_v2_agent_api_keys.sql"
    ))
    .execute(&db.pool)
    .await?;

    let rows: Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT id, revoked_at, revoked_reason FROM api_keys \
             WHERE id = ANY($1) ORDER BY id",
    )
    .bind(vec![
        user_v1_id,
        user_v2_id,
        live_v1_id,
        live_v2_id,
        malformed_v2_id,
        revoked_v1_id,
    ])
    .fetch_all(&db.pool)
    .await?;
    let row = |key_id| rows.iter().find(|row| row.0 == key_id).expect("key row");

    assert!(row(user_v1_id).1.is_some());
    assert_eq!(row(user_v1_id).2.as_deref(), Some("api_key_v2_cutover"));
    assert!(row(user_v2_id).1.is_some());
    assert_eq!(row(user_v2_id).2.as_deref(), Some("api_key_v2_cutover"));
    assert!(row(live_v1_id).1.is_some());
    assert_eq!(row(live_v1_id).2.as_deref(), Some("api_key_v2_cutover"));
    assert!(row(live_v2_id).1.is_some());
    assert_eq!(row(live_v2_id).2.as_deref(), Some("api_key_v2_cutover"));
    assert!(row(malformed_v2_id).1.is_some());
    assert_eq!(
        row(malformed_v2_id).2.as_deref(),
        Some("api_key_v2_cutover")
    );
    assert!(row(revoked_v1_id).1.is_some());
    assert_eq!(row(revoked_v1_id).2.as_deref(), Some("manual-revocation"));

    let events: Vec<AuditEventRow> = sqlx::query_as(
            "SELECT owner_user_id, actor_account_id, source, op_type, resource_type, resource_id, metadata->>'reason' AS reason \
             FROM audit_events WHERE resource_type = 'api_key' AND resource_id = ANY($1)",
        )
        .bind(vec![
            user_v1_id,
            user_v2_id,
            live_v1_id,
            live_v2_id,
            malformed_v2_id,
        ])
        .fetch_all(&db.pool)
        .await?;
    assert_eq!(events.len(), 5);
    for (key_id, op_type) in [
        (user_v1_id, "user_key.revoke"),
        (user_v2_id, "user_key.revoke"),
        (live_v1_id, "agent_key.revoke"),
        (live_v2_id, "agent_key.revoke"),
        (malformed_v2_id, "agent_key.revoke"),
    ] {
        let event = events
            .iter()
            .find(|event| event.resource_id == Some(key_id))
            .expect("key audit event");
        assert_eq!(event.owner_user_id, Some(owner));
        assert_eq!(event.actor_account_id, None);
        assert_eq!(event.source, "system");
        assert_eq!(event.op_type, op_type);
        assert_eq!(event.resource_type, "api_key");
        assert_eq!(event.reason, "api_key_v2_cutover");
    }

    let late_user_key_id = Uuid::new_v4();
    let late_user_key = repo
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id: late_user_key_id,
            account_id: owner,
            command: &CreateApiKey {
                name: "late-user-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &agent_api_key_prefix(late_user_key_id),
            token_hash: "hash-late-user",
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await
        .unwrap_err();
    assert!(
        late_user_key
            .to_string()
            .contains("api keys may only belong to agent accounts")
    );

    let late_legacy_key_id = Uuid::new_v4();
    let late_legacy_key = repo
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id: late_legacy_key_id,
            account_id: agent_id,
            command: &CreateApiKey {
                name: "late-legacy-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &format!("ngk_v1_{late_legacy_key_id}"),
            token_hash: "hash-late-legacy-agent",
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await
        .unwrap_err();
    assert!(
        late_legacy_key
            .to_string()
            .contains("agent api key prefix must match ngk_v2_{key_id}")
    );

    let late_malformed_key_id = Uuid::new_v4();
    let late_malformed_key = repo
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id: late_malformed_key_id,
            account_id: agent_id,
            command: &CreateApiKey {
                name: "late-malformed-key".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: "ngk_v2_malformed",
            token_hash: "hash-late-malformed-agent",
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await
        .unwrap_err();
    assert!(
        late_malformed_key
            .to_string()
            .contains("agent api key prefix must match ngk_v2_{key_id}")
    );

    let late_agent_key_id = Uuid::new_v4();
    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id: late_agent_key_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "late-agent-key".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(late_agent_key_id),
        token_hash: "hash-late-agent",
        created_by: owner,
        rotated_from_key_id: None,
    })
    .await?;

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
    let owner = insert_user_account(&db.pool, "rotate-user", "rotate@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, owner, "rotate-agent").await?;
    let first_key_id = Uuid::new_v4();

    for index in 0..TEST_API_KEYS_PER_ACCOUNT_MAX {
        let key_id = if index == 0 {
            first_key_id
        } else {
            Uuid::new_v4()
        };
        let token_hash = format!("hash-{index}");
        repo.insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id: agent_id,
            command: &CreateApiKey {
                name: format!("key-{index}"),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &agent_api_key_prefix(key_id),
            token_hash: &token_hash,
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await?;
    }
    assert_eq!(
        repo.count_live_keys(agent_id).await?,
        TEST_API_KEYS_PER_ACCOUNT_MAX
    );

    let new_key_id = Uuid::new_v4();
    let rotated = repo
        .rotate_key(
            InsertApiKey {
                key_id: new_key_id,
                account_id: agent_id,
                command: &CreateApiKey {
                    name: "key-0".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                },
                token_prefix: &agent_api_key_prefix(new_key_id),
                token_hash: "hash-rotated",
                created_by: owner,
                rotated_from_key_id: Some(first_key_id),
            },
            first_key_id,
            owner,
            TEST_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;

    assert_eq!(rotated.rotated_from_key_id, Some(first_key_id));
    assert_eq!(
        repo.count_live_keys(agent_id).await?,
        TEST_API_KEYS_PER_ACCOUNT_MAX
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

#[tokio::test]
async fn rotate_key_rejects_inactive_account() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let owner = insert_user_account(
        &db.pool,
        "inactive-rotate-user",
        "inactive-rotate@example.test",
    )
    .await?;
    let agent_id = insert_agent_account(&db.pool, owner, "inactive-rotate-agent").await?;
    let old_key_id = Uuid::new_v4();

    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id: old_key_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "old-key".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(old_key_id),
        token_hash: "hash-old-key",
        created_by: owner,
        rotated_from_key_id: None,
    })
    .await?;
    deactivate_account(&db.pool, agent_id, owner).await?;

    let command = CreateApiKey {
        name: "new-key".to_owned(),
        scopes: Vec::new(),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
    };
    let new_key_id = Uuid::new_v4();
    let err = repo
        .rotate_key(
            InsertApiKey {
                key_id: new_key_id,
                account_id: agent_id,
                command: &command,
                token_prefix: &agent_api_key_prefix(new_key_id),
                token_hash: "hash-new-key",
                created_by: owner,
                rotated_from_key_id: Some(old_key_id),
            },
            old_key_id,
            owner,
            TEST_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(message) if message == "account not found"));
    assert_eq!(repo.count_live_keys(agent_id).await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn live_agent_api_key_resolves_account_and_rejects_inactive_agent()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "agent-owner", "agent-owner@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, creator, "api-agent").await?;
    let key_id = Uuid::new_v4();
    let token_prefix = agent_api_key_prefix(key_id);

    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "agent-key".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &token_prefix,
        token_hash: "hash-agent-key",
        created_by: creator,
        rotated_from_key_id: None,
    })
    .await?;

    assert_eq!(
        repo.find_live_agent_id_by_key(key_id, &token_prefix, "hash-agent-key")
            .await?,
        Some(agent_id)
    );
    assert_eq!(
        repo.find_live_agent_id_by_key(key_id, "ngk_v2_other", "hash-agent-key")
            .await?,
        None
    );

    sqlx::query(
        "UPDATE accounts SET is_active = false, deleted_at = now(), deleted_by_account_id = $2 WHERE id = $1",
    )
    .bind(agent_id)
    .bind(creator)
    .execute(&db.pool)
    .await?;
    assert_eq!(
        repo.find_live_agent_id_by_key(key_id, &token_prefix, "hash-agent-key")
            .await?,
        None
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn live_key_lookup_rejects_revoked_and_expired_keys() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let user_id = insert_user_account(&db.pool, "lookup-user", "lookup@example.test").await?;
    let agent_id = insert_agent_account(&db.pool, user_id, "lookup-agent").await?;
    let live_id = Uuid::new_v4();
    let revoked_id = Uuid::new_v4();
    let expired_id = Uuid::new_v4();

    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id: live_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "live".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(live_id),
        token_hash: "hash-live",
        created_by: user_id,
        rotated_from_key_id: None,
    })
    .await?;
    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id: revoked_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "revoked".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(revoked_id),
        token_hash: "hash-revoked",
        created_by: user_id,
        rotated_from_key_id: None,
    })
    .await?;
    repo.revoke_key(agent_id, revoked_id, user_id, Some("test"))
        .await?;
    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id: expired_id,
        account_id: agent_id,
        command: &CreateApiKey {
            name: "expired".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(expired_id),
        token_hash: "hash-expired",
        created_by: user_id,
        rotated_from_key_id: None,
    })
    .await?;
    // Create with a valid future expiry, then expire it directly. The create-time
    // guard rejects past expiries, but a validly-created key can later expire.
    sqlx::query("UPDATE api_keys SET expires_at = now() - interval '1 hour' WHERE id = $1")
        .bind(expired_id)
        .execute(&db.pool)
        .await?;

    assert_eq!(
        repo.find_live_agent_id_by_key(live_id, &agent_api_key_prefix(live_id), "hash-live")
            .await?,
        Some(agent_id)
    );
    assert_eq!(
        repo.find_live_agent_id_by_key(
            revoked_id,
            &agent_api_key_prefix(revoked_id),
            "hash-revoked",
        )
        .await?,
        None
    );
    assert_eq!(
        repo.find_live_agent_id_by_key(
            expired_id,
            &agent_api_key_prefix(expired_id),
            "hash-expired",
        )
        .await?,
        None
    );
    assert_eq!(repo.count_live_keys(agent_id).await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn revoke_key_is_scoped_to_account_id() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = ApiKeyRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "owner@example.test").await?;
    let other_owner = insert_user_account(&db.pool, "other", "other@example.test").await?;
    let owner_agent = insert_agent_account(&db.pool, owner, "owner-agent").await?;
    let other_agent = insert_agent_account(&db.pool, other_owner, "other-agent").await?;
    let key_id = Uuid::new_v4();

    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id,
        account_id: other_agent,
        command: &CreateApiKey {
            name: "other-key".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
        },
        token_prefix: &agent_api_key_prefix(key_id),
        token_hash: "hash-other",
        created_by: other_owner,
        rotated_from_key_id: None,
    })
    .await?;

    let result = repo
        .revoke_key(owner_agent, key_id, owner, Some("test"))
        .await;
    assert!(result.is_err(), "wrong account id cannot revoke the key");

    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(revoked_at.is_none());

    repo.revoke_key(other_agent, key_id, other_owner, Some("test"))
        .await?;
    assert_eq!(repo.count_live_keys(other_agent).await?, 0);

    db.cleanup().await;
    Ok(())
}
