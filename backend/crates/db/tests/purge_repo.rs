//! Integration tests for soft-delete hard purge.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, agent_api_key_prefix, insert_user_account};
use notegate_core::security::PiiCrypto;
use notegate_db::{
    ApiKeyRepo, BrowserSessionRepo, PurgeRepo, api_key_repo::InsertApiKey,
    browser_session_repo::InsertBrowserSession,
};
use notegate_model::CreateApiKey;
use sqlx::Row as _;
use uuid::Uuid;

static PURGE_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn purge_deletes_due_spaces_and_nodes() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "purger", "purger@example.test").await?;

    let due_space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name, deleted_at, deleted_by_user_id, purge_after) \
         VALUES ($1, 'due-space', now() - interval '40 days', $1, now() - interval '1 day') \
         RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;

    let live_space: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'live-space') RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
            .bind(live_space)
            .fetch_one(&db.pool)
            .await?;
    let due_node: Uuid = sqlx::query_scalar(
        "INSERT INTO nodes \
         (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id, deleted_by_account_id, deleted_at, purge_after) \
         VALUES ($1, $2, 'old.md', 'text', $3, $3, $3, now() - interval '40 days', now() - interval '1 day') \
         RETURNING id",
    )
    .bind(live_space)
    .bind(root)
    .bind(user)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO text_objects \
         (node_id, space_id, content_text, content_sha256, byte_len, line_count, media_type, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, 'old', $3, 3, 1, 'text/plain', $4, $4)",
    )
    .bind(due_node)
    .bind(live_space)
    .bind("2".repeat(64))
    .bind(user)
    .execute(&db.pool)
    .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.spaces_deleted, 1);
    assert_eq!(run.nodes_deleted, 1);

    let space_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM spaces WHERE id = $1")
        .bind(due_space)
        .fetch_optional(&db.pool)
        .await?;
    assert!(space_exists.is_none());

    let node_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM nodes WHERE id = $1")
        .bind(due_node)
        .fetch_optional(&db.pool)
        .await?;
    assert!(node_exists.is_none());

    let text_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT node_id FROM text_objects WHERE node_id = $1")
            .bind(due_node)
            .fetch_optional(&db.pool)
            .await?;
    assert!(text_exists.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_deletes_expired_mcp_invocations_in_bounded_batches()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "mcp-purger", "mcp-purger@example.test").await?;

    sqlx::query(
        "INSERT INTO mcp_invocations \
         (created_at, owner_user_id, actor_account_id, caller_kind, tool, op, purpose, input, outcome, duration_ms) \
         SELECT now() - interval '91 days', $1, $1, 'user', 'search', 'find', \
                'expired invocation ' || value, '{}'::jsonb, 'success', 1 \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO mcp_invocations \
         (created_at, owner_user_id, actor_account_id, caller_kind, tool, op, purpose, input, outcome, duration_ms) \
         VALUES (now() - interval '89 days', $1, $1, 'user', 'read', 'read', \
                 'recent invocation', '{}'::jsonb, 'success', 1)",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;

    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.mcp_invocations_deleted, 1_000);
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.mcp_invocations_deleted, 1);

    let remaining: Vec<String> = sqlx::query_scalar(
        "SELECT purpose FROM mcp_invocations WHERE owner_user_id = $1 ORDER BY id",
    )
    .bind(user)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(remaining, vec!["recent invocation"]);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_deletes_expired_event_history_in_bounded_batches()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "event-purger", "event-purger@example.test").await?;
    let space_id: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'event-purge-space') RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;

    sqlx::query(
        "INSERT INTO audit_events \
         (created_at, owner_user_id, actor_account_id, source, op_type, resource_type, metadata) \
         SELECT now() - interval '366 days', $1, $1, 'rest', 'test.expired', 'test', \
                jsonb_build_object('sequence', value) \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO audit_events \
         (created_at, owner_user_id, actor_account_id, source, op_type, resource_type) \
         VALUES (now() - interval '364 days', $1, $1, 'rest', 'test.recent', 'test')",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;

    sqlx::query(
        "INSERT INTO file_change_events \
         (created_at, space_id, actor_account_id, op_type, metadata) \
         SELECT now() - interval '91 days', $1, $2, 'test.expired', \
                jsonb_build_object('sequence', value) \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(space_id)
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO file_change_events \
         (created_at, space_id, actor_account_id, op_type) \
         VALUES (now() - interval '89 days', $1, $2, 'test.recent')",
    )
    .bind(space_id)
    .bind(user)
    .execute(&db.pool)
    .await?;

    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.audit_events_deleted, 1_000);
    assert_eq!(first.file_change_events_deleted, 1_000);
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.audit_events_deleted, 1);
    assert_eq!(second.file_change_events_deleted, 1);

    let remaining_audit_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE op_type = 'test.recent'")
            .fetch_one(&db.pool)
            .await?;
    let remaining_file_change_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_change_events WHERE op_type = 'test.recent'")
            .fetch_one(&db.pool)
            .await?;
    let expired_audit_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE op_type = 'test.expired'")
            .fetch_one(&db.pool)
            .await?;
    let expired_file_change_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_change_events WHERE op_type = 'test.expired'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(remaining_audit_events, 1);
    assert_eq!(remaining_file_change_events, 1);
    assert_eq!(expired_audit_events, 0);
    assert_eq!(expired_file_change_events, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_deletes_terminal_object_history_in_bounded_batches()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let object_key_prefix = format!("objects/retention-{}/", Uuid::new_v4());

    sqlx::query(
        "INSERT INTO object_storage_objects \
         (id, object_key, name, declared_byte_len, media_type, state, last_activity_at, deleted_at) \
         SELECT gen_random_uuid(), $1 || value::text, 'expired.bin', 1, \
                'application/octet-stream', \
                CASE WHEN value % 2 = 0 THEN 'expired' ELSE 'deleted' END, \
                now() - interval '91 days', now() - interval '91 days' \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(&object_key_prefix)
    .execute(&db.pool)
    .await?;
    let recent_object_key = format!("{object_key_prefix}recent");
    sqlx::query(
        "INSERT INTO object_storage_objects \
         (id, object_key, name, declared_byte_len, media_type, state, last_activity_at, deleted_at) \
         VALUES (gen_random_uuid(), $1, 'recent.bin', 1, 'application/octet-stream', \
                 'deleted', now() - interval '89 days', now() - interval '89 days')",
    )
    .bind(&recent_object_key)
    .execute(&db.pool)
    .await?;

    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.object_storage_history_deleted, 1_000);
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.object_storage_history_deleted, 1);

    let old_remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_storage_objects \
         WHERE object_key LIKE $1 AND object_key <> $2",
    )
    .bind(format!("{object_key_prefix}%"))
    .bind(&recent_object_key)
    .fetch_one(&db.pool)
    .await?;
    let recent_remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_storage_objects WHERE object_key = $1")
            .bind(&recent_object_key)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(old_remaining, 0);
    assert_eq!(recent_remaining, 1);

    db.cleanup().await;
    Ok(())
}

/// Seed one live key via the repo, returning its id.
async fn seed_key(
    repo: &ApiKeyRepo,
    account_id: Uuid,
    created_by: Uuid,
    name: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let key_id = Uuid::new_v4();
    let key = repo
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id,
            command: &CreateApiKey {
                name: name.to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: &agent_api_key_prefix(key_id),
            token_hash: &format!("hash-{name}-{}", Uuid::new_v4()),
            created_by,
            rotated_from_key_id: None,
        })
        .await?;
    Ok(key.id)
}

async fn seed_browser_session(
    repo: &BrowserSessionRepo,
    crypto: &PiiCrypto,
    user_id: Uuid,
    name: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let session_id = Uuid::new_v4();
    let token_hash = crypto.browser_session_hash(&session_id.to_string(), name)?;
    let refresh_token = crypto
        .encrypt_browser_refresh_token(&session_id.to_string(), &format!("refresh-{name}"))?;
    repo.insert_session(InsertBrowserSession {
        session_id,
        user_id,
        token_prefix: "ngs_v1_test",
        token_hash: &token_hash,
        refresh_token: &refresh_token,
        refresh_token_enc_key_id: crypto.enc_key_id(),
        refresh_token_enc_version: crypto.version(),
        validated_until: chrono::Utc::now() + chrono::Duration::hours(1),
        expires_at: chrono::Utc::now() + chrono::Duration::days(15),
    })
    .await?;
    Ok(session_id)
}

#[tokio::test]
async fn purge_deletes_long_dead_api_keys_only() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "key-purger", "key-purger@example.test").await?;
    let agent: Uuid =
        sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
            .fetch_one(&db.pool)
            .await?;
    sqlx::query("INSERT INTO agents (id, name, owner_user_id) VALUES ($1, 'key-purger', $2)")
        .bind(agent)
        .bind(user)
        .execute(&db.pool)
        .await?;
    let repo = ApiKeyRepo::new(db.pool.clone());

    // A key dies at the earlier of its revoke time and expiry. Retention is 30 days.
    let live = seed_key(&repo, agent, user, "live").await?;
    let old_revoked = seed_key(&repo, agent, user, "old-revoked").await?;
    let old_expired = seed_key(&repo, agent, user, "old-expired").await?;
    let recent_revoked = seed_key(&repo, agent, user, "recent-revoked").await?;

    sqlx::query(
        "UPDATE api_keys SET revoked_at = now() - interval '40 days', revoked_by_user_id = $2, \
         revoked_reason = 'test' WHERE id = $1",
    )
    .bind(old_revoked)
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query("UPDATE api_keys SET expires_at = now() - interval '40 days' WHERE id = $1")
        .bind(old_expired)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "UPDATE api_keys SET revoked_at = now() - interval '1 day', revoked_by_user_id = $2, \
         revoked_reason = 'test' WHERE id = $1",
    )
    .bind(recent_revoked)
    .bind(user)
    .execute(&db.pool)
    .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.api_keys_deleted, 2, "only the two long-dead keys purge");

    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM api_keys WHERE account_id = $1")
        .bind(agent)
        .fetch_all(&db.pool)
        .await?;
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(&live), "live key is retained");
    assert!(
        remaining.contains(&recent_revoked),
        "recently revoked key is within retention"
    );
    assert!(!remaining.contains(&old_revoked));
    assert!(!remaining.contains(&old_expired));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_deletes_long_dead_browser_sessions_only() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user =
        insert_user_account(&db.pool, "session-purger", "session-purger@example.test").await?;
    let repo = BrowserSessionRepo::new(db.pool.clone());
    let crypto = PiiCrypto::test();

    let live = seed_browser_session(&repo, &crypto, user, "live").await?;
    let old_revoked = seed_browser_session(&repo, &crypto, user, "old-revoked").await?;
    let old_expired = seed_browser_session(&repo, &crypto, user, "old-expired").await?;
    let recent_revoked = seed_browser_session(&repo, &crypto, user, "recent-revoked").await?;

    sqlx::query(
        "UPDATE browser_sessions SET revoked_at = now() - interval '40 days', \
         revoked_reason = 'test' WHERE id = $1",
    )
    .bind(old_revoked)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE browser_sessions SET expires_at = now() - interval '40 days', \
         validated_until = now() - interval '40 days' WHERE id = $1",
    )
    .bind(old_expired)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE browser_sessions SET revoked_at = now() - interval '1 day', \
         revoked_reason = 'test' WHERE id = $1",
    )
    .bind(recent_revoked)
    .execute(&db.pool)
    .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(
        run.browser_sessions_deleted, 2,
        "only the two long-dead sessions purge"
    );

    let remaining: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM browser_sessions WHERE user_id = $1")
            .bind(user)
            .fetch_all(&db.pool)
            .await?;
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(&live), "live session is retained");
    assert!(
        remaining.contains(&recent_revoked),
        "recently revoked session is within retention"
    );
    assert!(!remaining.contains(&old_revoked));
    assert!(!remaining.contains(&old_expired));

    db.cleanup().await;
    Ok(())
}
