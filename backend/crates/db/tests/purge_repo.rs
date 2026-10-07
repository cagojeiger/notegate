//! Integration tests for soft-delete hard purge.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, agent_api_key_prefix, attach_file, insert_user_account, space_with_root};
use notegate_core::security::PiiCrypto;
use notegate_db::{
    ApiKeyRepo, BrowserSessionRepo, PurgeRepo, api_key_repo::InsertApiKey,
    browser_session_repo::InsertBrowserSession,
};
use notegate_model::CreateApiKey;
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
    sqlx::query(
        "INSERT INTO node_link_projections (space_id, source_node_id) \
         VALUES ($1, gen_random_uuid()), ($2, $3)",
    )
    .bind(due_space)
    .bind(live_space)
    .bind(due_node)
    .execute(&db.pool)
    .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.spaces_deleted, 1);
    assert_eq!(run.nodes_deleted, 1);
    assert_eq!(run.link_graph_projections_deleted, 2);

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

    let orphaned_link_projections: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM node_link_projections \
         WHERE space_id = $1 OR source_node_id = $2",
    )
    .bind(due_space)
    .bind(due_node)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(orphaned_link_projections, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_deletes_expired_command_invocations_in_bounded_batches()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user =
        insert_user_account(&db.pool, "command-purger", "command-purger@example.test").await?;

    sqlx::query(
        "INSERT INTO command_invocations \
         (created_at, owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, input, outcome, duration_ms) \
         SELECT now() - interval '91 days', $1, $1, 'user', \
                CASE WHEN value % 2 = 0 THEN 'mcp' ELSE 'cli' END, 'search', 'find', \
                'expired invocation ' || value, '{}'::jsonb, 'success', 1 \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO command_invocations \
         (created_at, owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, input, outcome, duration_ms) \
         VALUES (now() - interval '89 days', $1, $1, 'user', 'cli', 'read', 'read', \
                 'recent invocation', '{}'::jsonb, 'success', 1)",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;

    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.command_invocations_deleted, 1_000);
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.command_invocations_deleted, 1);

    let remaining: Vec<String> = sqlx::query_scalar(
        "SELECT purpose FROM command_invocations WHERE owner_user_id = $1 ORDER BY id",
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
         SELECT now() - interval '181 days', $1, $1, 'rest', 'test.expired', 'test', \
                jsonb_build_object('sequence', value) \
         FROM generate_series(1, 1001) AS value",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO audit_events \
         (created_at, owner_user_id, actor_account_id, source, op_type, resource_type) \
         VALUES (now() - interval '179 days', $1, $1, 'rest', 'test.recent', 'test')",
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
                now() - interval '181 days', now() - interval '181 days' \
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
                 'deleted', now() - interval '179 days', now() - interval '179 days')",
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

#[tokio::test]
async fn history_retention_uses_injected_boundaries_and_keeps_pending_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, _) = space_with_root(&db.pool, "history-boundary").await?;
    let now =
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")?.with_timezone(&chrono::Utc);
    let audit_cutoff = now - chrono::Duration::days(180);
    let ordinary_cutoff = now - chrono::Duration::days(90);
    let mut audits = Vec::new();
    let mut changes = Vec::new();
    let mut invocations = Vec::new();
    let mut objects = Vec::new();
    for offset in [-1, 0, 1] {
        let delta = chrono::Duration::microseconds(offset);
        audits.push(sqlx::query_scalar::<_, i64>(
            "INSERT INTO audit_events (created_at, owner_user_id, actor_account_id, source, op_type, resource_type) \
             VALUES ($1, $2, $2, 'system', 'test.boundary', 'test') RETURNING id",
        ).bind(audit_cutoff + delta).bind(owner).fetch_one(&db.pool).await?);
        changes.push(
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO file_change_events (created_at, space_id, actor_account_id, op_type) \
             VALUES ($1, $2, $3, 'test.boundary') RETURNING id",
            )
            .bind(ordinary_cutoff + delta)
            .bind(space)
            .bind(owner)
            .fetch_one(&db.pool)
            .await?,
        );
        invocations.push(sqlx::query_scalar::<_, i64>(
            "INSERT INTO command_invocations (created_at, owner_user_id, actor_account_id, caller_kind, surface, tool, input, outcome, duration_ms) \
             VALUES ($1, $2, $2, 'user', 'mcp', 'read', '{}', 'success', 0) RETURNING id",
        ).bind(ordinary_cutoff + delta).bind(owner).fetch_one(&db.pool).await?);
        let object = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO object_storage_objects (id, object_key, name, declared_byte_len, media_type, state, deleted_at) \
             VALUES ($1, $2, 'boundary.bin', 1, 'application/octet-stream', 'deleted', $3)",
        ).bind(object).bind(format!("objects/{object}")).bind(audit_cutoff + delta)
            .execute(&db.pool).await?;
        objects.push(object);
    }
    let pending = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO object_storage_objects (id, object_key, name, declared_byte_len, media_type, state, last_activity_at) \
         VALUES ($1, $2, 'pending.bin', 1, 'application/octet-stream', 'delete_pending', $3)",
    ).bind(pending).bind(format!("objects/{pending}")).bind(audit_cutoff - chrono::Duration::days(1))
        .execute(&db.pool).await?;

    let repo = PurgeRepo::new(db.pool.clone()).with_history_time(now);
    let run = repo.run_once().await?;
    assert_eq!(run.audit_events_deleted, 2);
    assert_eq!(run.file_change_events_deleted, 2);
    assert_eq!(run.command_invocations_deleted, 2);
    assert_eq!(run.object_storage_history_deleted, 2);
    assert_eq!(
        sqlx::query_scalar::<_, Vec<i64>>(
            "SELECT array_agg(id ORDER BY id) FROM audit_events WHERE id = ANY($1)",
        )
        .bind(&audits)
        .fetch_one(&db.pool)
        .await?,
        vec![audits[2]]
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<i64>>(
            "SELECT array_agg(id ORDER BY id) FROM file_change_events WHERE id = ANY($1)",
        )
        .bind(&changes)
        .fetch_one(&db.pool)
        .await?,
        vec![changes[2]]
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<i64>>(
            "SELECT array_agg(id ORDER BY id) FROM command_invocations WHERE id = ANY($1)",
        )
        .bind(&invocations)
        .fetch_one(&db.pool)
        .await?,
        vec![invocations[2]]
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<Uuid>>(
            "SELECT array_agg(id) FROM object_storage_objects WHERE id = ANY($1)",
        )
        .bind(&objects)
        .fetch_one(&db.pool)
        .await?,
        vec![objects[2]]
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(pending)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(state, "delete_pending");
    let repeated = repo.run_once().await?;
    assert_eq!(
        repeated.audit_events_deleted
            + repeated.file_change_events_deleted
            + repeated.command_invocations_deleted
            + repeated.object_storage_history_deleted,
        0
    );
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

#[tokio::test]
async fn resource_failure_rolls_back_deletion_intent_but_allows_other_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    assert_purge_failure_isolation(&["resources"]).await
}

#[tokio::test]
async fn identity_failure_preserves_resources_and_allows_history_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    assert_purge_failure_isolation(&["identities"]).await
}

#[tokio::test]
async fn history_failure_does_not_undo_resources_or_identities()
-> Result<(), Box<dyn std::error::Error>> {
    assert_purge_failure_isolation(&["history"]).await
}

#[tokio::test]
async fn multiple_group_failures_still_allow_history_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    assert_purge_failure_isolation(&["resources", "identities"]).await
}

async fn assert_purge_failure_isolation(
    failed_groups: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (user, space, root) = space_with_root(&db.pool, "purge-isolation").await?;
    let files = notegate_db::FilesRepo::new(db.pool.clone());
    let (node, _) = attach_file(&files, space, root, "retired.bin", 10, user).await?;
    let object: Uuid =
        sqlx::query_scalar("SELECT id FROM object_storage_objects WHERE node_id = $1")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    // Leave the object attached to exercise purge's safety net for missed
    // soft-delete requests, including rollback if a later resource query fails.
    sqlx::query(
        "UPDATE spaces SET deleted_at = now() - interval '40 days', \
         deleted_by_user_id = $2, purge_after = now() - interval '1 day' WHERE id = $1",
    )
    .bind(space)
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "UPDATE accounts SET is_active = false, deleted_at = now() - interval '40 days', \
         deleted_by_account_id = id WHERE id = $1",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO audit_events \
         (created_at, owner_user_id, actor_account_id, source, op_type, resource_type) \
         VALUES (now() - interval '181 days', $1, $1, 'system', 'test.expired', 'test')",
    )
    .bind(user)
    .execute(&db.pool)
    .await?;

    sqlx::raw_sql(
        "CREATE FUNCTION fail_purge_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'injected purge failure'; END; $$",
    )
    .execute(&db.pool)
    .await?;
    // Fail after preceding statements have already changed rows within each
    // group. Statement triggers also fire when the failing table is empty.
    let failure_tables = [
        ("resources", "node_link_projections"),
        ("identities", "browser_sessions"),
        ("history", "file_change_events"),
    ];
    for (group, table) in failure_tables {
        if failed_groups.contains(&group) {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE TRIGGER fail_purge BEFORE DELETE ON {table} \
                 FOR EACH STATEMENT EXECUTE FUNCTION fail_purge_test()"
            )))
            .execute(&db.pool)
            .await?;
        }
    }

    let repo = PurgeRepo::new(db.pool.clone());
    assert!(
        repo.run_once().await.is_err(),
        "partial success is an error"
    );

    let resources_failed = failed_groups.contains(&"resources");
    let identities_failed = failed_groups.contains(&"identities");
    let history_failed = failed_groups.contains(&"history");
    let space_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1)")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(space_exists, resources_failed);
    let ledger: (String, Option<Uuid>, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT state, space_id, node_id, delete_requested_at IS NOT NULL \
         FROM object_storage_objects WHERE id = $1",
    )
    .bind(object)
    .fetch_one(&db.pool)
    .await?;
    if resources_failed {
        assert_eq!(
            ledger,
            ("attached".to_owned(), Some(space), Some(node.id), false)
        );
    } else {
        assert_eq!(ledger, ("delete_pending".to_owned(), None, None, true));
    }
    let identity: (bool, bool) = sqlx::query_as(
        "SELECT u.anonymized_at IS NOT NULL, a.display_name_ciphertext IS NULL \
         FROM users u JOIN accounts a ON a.id = u.id WHERE u.id = $1",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(identity, (!identities_failed, !identities_failed));
    let expired_audit_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM audit_events WHERE op_type = 'test.expired')",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(expired_audit_exists, history_failed);

    // The next scheduled attempt must converge without redoing committed work.
    for (group, table) in failure_tables {
        if failed_groups.contains(&group) {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP TRIGGER fail_purge ON {table}"
            )))
            .execute(&db.pool)
            .await?;
        }
    }
    let retry = repo.run_once().await?;
    assert_eq!(retry.spaces_deleted, u64::from(resources_failed));
    assert_eq!(retry.object_deletions_queued, u64::from(resources_failed));
    assert_eq!(retry.accounts_anonymized, u64::from(identities_failed));
    assert_eq!(retry.audit_events_deleted, u64::from(history_failed));
    let settled = repo.run_once().await?;
    assert_eq!(settled.spaces_deleted, 0);
    assert_eq!(settled.object_deletions_queued, 0);
    assert_eq!(settled.accounts_anonymized, 0);
    assert_eq!(settled.audit_events_deleted, 0);

    db.cleanup().await;
    Ok(())
}

// Raw fixtures keep large-tree tests fast while representing retained physical
// rows rather than using the live-subtree delete API's 1,000-node request cap.
async fn retained_node(
    pool: &sqlx::PgPool,
    space: Uuid,
    parent: Uuid,
    owner: Uuid,
    kind: &str,
    due: bool,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO nodes (id, space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id, \
             deleted_by_account_id, deleted_at, purge_after, deletion_target_node_id, deletion_operation_id) \
         SELECT id, $1, $2, id::text, $4, $3, $3, $3, now() - interval '1 day', \
             CASE WHEN $5 THEN now() - interval '1 second' ELSE now() + interval '29 days' END, id, gen_random_uuid() \
         FROM (SELECT gen_random_uuid() AS id) seed RETURNING id",
    ).bind(space).bind(parent).bind(owner).bind(kind).bind(due).fetch_one(pool).await
}

async fn retained_children(
    pool: &sqlx::PgPool,
    space: Uuid,
    parent: Uuid,
    owner: Uuid,
    count: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO nodes (id, space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id, \
             deleted_by_account_id, deleted_at, purge_after, deletion_target_node_id, deletion_operation_id) \
         SELECT id, $1, $2, 'retained-' || value, 'folder', $3, $3, $3, now() - interval '1 day', \
             now() + interval '29 days', id, gen_random_uuid() \
         FROM (SELECT gen_random_uuid() AS id, value FROM generate_series(1, $4) value) seed",
    ).bind(space).bind(parent).bind(owner).bind(count).execute(pool).await?;
    Ok(())
}

#[tokio::test]
async fn large_physical_subtree_drains_children_before_parent_and_survives_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "purge-tree-batches").await?;
    let parent = retained_node(&db.pool, space, root, owner, "folder", true).await?;
    // These children's own deadlines are in the future. The irreversible
    // ancestor still owns their physical deletion, without restoring them.
    retained_children(&db.pool, space, parent, owner, 205).await?;
    let (other_owner, other_space, other_root) =
        space_with_root(&db.pool, "purge-small-peer").await?;
    retained_node(
        &db.pool,
        other_space,
        other_root,
        other_owner,
        "folder",
        true,
    )
    .await?;

    for (expected, remaining) in [(101, 105), (100, 5), (5, 0), (1, 0)] {
        // A fresh instance on every pass cannot rely on an in-memory cursor.
        let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
        assert_eq!(run.nodes_deleted, expected);
        let children: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE parent_id = $1")
            .bind(parent)
            .fetch_one(&db.pool)
            .await?;
        assert_eq!(children, remaining);
        let parent_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE id = $1)")
                .bind(parent)
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(parent_exists, expected != 1);
        assert_eq!(run.resources_pending, expected != 1);
    }
    let live_usage: i64 =
        sqlx::query_scalar("SELECT live_node_count FROM space_usage WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(
        live_usage, 1,
        "hard deletion does not release live quota twice"
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn hard_purge_drains_revisions_without_cascading_or_losing_usage_accounting()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "purge-revision-batches").await?;
    let node = retained_node(&db.pool, space, root, owner, "text", true).await?;
    sqlx::query(
        "INSERT INTO text_objects (node_id, space_id, content_text, content_sha256, byte_len, line_count, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, 'current', $3, 7, 1, $4, $4)",
    ).bind(node).bind(space).bind("a".repeat(64)).bind(owner).execute(&db.pool).await?;
    sqlx::query(
        "INSERT INTO text_revisions (id, node_id, space_id, content_sha256, byte_len, line_count, written_at, \
             author_id, group_id, source, checkpoint, superseded_at, cleanup_at, ciphertext, nonce, enc_key_id, enc_version) \
         SELECT gen_random_uuid(), $1, $2, $3, 1, 1, now(), $4, gen_random_uuid(), 'browser', true, now(), \
             now() + interval '30 days', decode('aa', 'hex'), decode('bb', 'hex'), 'fixture', 1 \
         FROM generate_series(1, 201)",
    ).bind(node).bind(space).bind("b".repeat(64)).bind(owner).execute(&db.pool).await?;
    sqlx::query("INSERT INTO text_revision_usage(space_id, stored_bytes) VALUES ($1, 402)")
        .bind(space)
        .execute(&db.pool)
        .await?;
    let version: (chrono::DateTime<chrono::Utc>, Option<Uuid>) =
        sqlx::query_as("SELECT deleted_at, deletion_operation_id FROM nodes WHERE id = $1")
            .bind(node)
            .fetch_one(&db.pool)
            .await?;

    for (deleted, remaining, node_exists) in [(100, 101, true), (100, 1, true), (1, 0, false)] {
        let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
        assert_eq!(run.text_revisions_deleted, deleted);
        assert_eq!(run.nodes_deleted, u64::from(!node_exists));
        let stored: (i64, i64, bool) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM text_revisions WHERE node_id = $1), \
             (SELECT stored_bytes FROM text_revision_usage WHERE space_id = $2), \
             EXISTS(SELECT 1 FROM text_objects WHERE node_id = $1 AND content_text = 'current')",
        )
        .bind(node)
        .bind(space)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(stored, (remaining, remaining * 2, node_exists));
        assert_eq!(run.resources_pending, node_exists);
        if node_exists {
            let error = notegate_db::FilesRepo::new(db.pool.clone())
                .restore_trashed_node(
                    owner,
                    space,
                    node,
                    notegate_model::trash::TrashEntryVersion {
                        deleted_at: version.0,
                        deletion_operation_id: version.1,
                    },
                )
                .await
                .expect_err("partially purged expired content cannot be restored");
            assert!(matches!(error, notegate_core::Error::Conflict(_)));
        }
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn persisted_rotation_reaches_spaces_beyond_the_candidate_window()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "purge-rotation", "rotation@example.test").await?;
    let mut spaces = Vec::new();
    for index in 0..11 {
        let space: Uuid = sqlx::query_scalar(
            "INSERT INTO spaces(owner_user_id, name) VALUES ($1, $2) RETURNING id",
        )
        .bind(owner)
        .bind(format!("rotate-{index}"))
        .fetch_one(&db.pool)
        .await?;
        let root: Uuid =
            sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
                .bind(space)
                .fetch_one(&db.pool)
                .await?;
        retained_children(&db.pool, space, root, owner, 101).await?;
        sqlx::query("UPDATE spaces SET deleted_at = now(), deleted_by_user_id = $2, purge_after = now() - interval '1 second' WHERE id = $1")
            .bind(space).bind(owner).execute(&db.pool).await?;
        spaces.push(space);
    }
    spaces.sort_unstable();
    let last = spaces[10];
    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.nodes_deleted, 1_000);
    let first_last: (i64, bool) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM nodes WHERE space_id = $1 AND parent_id IS NOT NULL), \
         purge_last_attempt_at IS NULL FROM spaces WHERE id = $1",
    )
    .bind(last)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(first_last, (101, true));
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.nodes_deleted, 109);
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE space_id = $1 AND parent_id IS NOT NULL",
    )
    .bind(last)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        remaining, 1,
        "the untouched Space precedes previously attempted Spaces"
    );
    assert!(second.resources_pending);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn busy_space_does_not_block_other_spaces_and_resumes_after_unlock()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, busy, root) = space_with_root(&db.pool, "purge-busy").await?;
    let node = retained_node(&db.pool, busy, root, owner, "folder", true).await?;
    let (other_owner, other, other_root) = space_with_root(&db.pool, "purge-free").await?;
    retained_node(&db.pool, other, other_root, other_owner, "folder", true).await?;
    let mut lock = db.pool.begin().await?;
    // Production shared mutation gate, scoped to this test schema.
    let value = busy.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ 0x4e47_5350_4143_4501;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended(current_schema(), $1))")
        .bind(i64::from_ne_bytes(folded.to_ne_bytes()))
        .execute(&mut *lock)
        .await?;
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        PurgeRepo::new(db.pool.clone()).run_once(),
    )
    .await??;
    assert_eq!(first.nodes_deleted, 1);
    assert!(first.resources_pending);
    let retained: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE id = $1)")
        .bind(node)
        .fetch_one(&db.pool)
        .await?;
    assert!(retained);
    lock.commit().await?;
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .nodes_deleted,
        1
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn pending_uploads_are_expired_in_batches_before_their_folder_is_removed()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "purge-upload-batches").await?;
    let parent = retained_node(&db.pool, space, root, owner, "folder", true).await?;
    sqlx::query(
        "INSERT INTO object_storage_objects(id, object_key, space_id, parent_node_id, requested_by_account_id, name, declared_byte_len, media_type, state) \
         SELECT id, id::text, $1, $2, $3, 'pending.bin', 1, 'application/octet-stream', 'uploading' \
         FROM (SELECT gen_random_uuid() AS id FROM generate_series(1, 201)) seed",
    ).bind(space).bind(parent).bind(owner).execute(&db.pool).await?;
    for (queued, remaining) in [(100, 101), (100, 1), (1, 0)] {
        let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
        assert_eq!(run.object_deletions_queued, queued);
        assert_eq!(run.nodes_deleted, u64::from(remaining == 0));
        let anchors: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM object_storage_objects WHERE parent_node_id = $1",
        )
        .bind(parent)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(anchors, remaining);
        let expired: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM object_storage_objects f WHERE f.space_id = $1 AND f.state = 'expire_pending' \
             AND f.parent_node_id IS NULL AND f.deletion_operation_id IS NOT NULL",
        ).bind(space).fetch_one(&db.pool).await?;
        assert_eq!(expired, 201 - remaining);
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn link_references_drain_in_batches_without_losing_incoming_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "purge-link-batches").await?;
    let files = notegate_db::FilesRepo::new(db.pool.clone());
    let (file_node, file) = attach_file(&files, space, root, "target.bin", 1, owner).await?;
    files
        .soft_delete_node(space, file_node.id, owner, false)
        .await?;
    sqlx::query("UPDATE nodes SET purge_after = now(), purge_requested_at = now() WHERE id = $1")
        .bind(file_node.id)
        .execute(&db.pool)
        .await?;
    let target = file_node.id;
    let outgoing = retained_node(&db.pool, space, root, owner, "text", true).await?;
    let source: Uuid = sqlx::query_scalar(
        "INSERT INTO nodes(space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, 'live.md', 'text', $3, $3) RETURNING id",
    ).bind(space).bind(root).bind(owner).fetch_one(&db.pool).await?;
    sqlx::query(
        "INSERT INTO node_link_refs(space_id, source_node_id, target_node_id, target_path, reference_kind, occurrence_count) \
         SELECT $1, $2, $3, '/incoming-' || value, 'link', 1 FROM generate_series(1, 1001) value \
         UNION ALL SELECT $1, $4, NULL::uuid, '/outgoing-' || value, 'link', 1 FROM generate_series(1, 1001) value",
    ).bind(space).bind(source).bind(target).bind(outgoing).execute(&db.pool).await?;
    let first = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(first.nodes_deleted, 0);
    assert_eq!(first.object_deletions_queued, 0);
    let refs: (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE source_node_id = $1), count(*) FILTER (WHERE target_node_id = $2) FROM node_link_refs",
    ).bind(outgoing).bind(target).fetch_one(&db.pool).await?;
    assert_eq!(refs, (1, 1));
    let retained: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state, node_id FROM object_storage_objects WHERE object_key = $1")
            .bind(&file.object_key)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(retained, ("attached".to_owned(), Some(target)));
    let second = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(second.nodes_deleted, 2);
    assert_eq!(second.object_deletions_queued, 1);
    let removed: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state, node_id FROM object_storage_objects WHERE object_key = $1")
            .bind(&file.object_key)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(removed, ("delete_pending".to_owned(), None));
    let paths: (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE target_node_id IS NOT NULL) FROM node_link_refs WHERE source_node_id = $1",
    ).bind(source).fetch_one(&db.pool).await?;
    assert_eq!(paths, (1001, 0));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failed_space_batch_preserves_its_intent_but_not_other_spaces_committed_progress()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, bad, bad_root) = space_with_root(&db.pool, "purge-failed-space").await?;
    let (good_owner, good, good_root) = space_with_root(&db.pool, "purge-successful-space").await?;
    let files = notegate_db::FilesRepo::new(db.pool.clone());
    let (bad_node, bad_file) = attach_file(&files, bad, bad_root, "bad.bin", 1, owner).await?;
    let (good_node, good_file) =
        attach_file(&files, good, good_root, "good.bin", 1, good_owner).await?;
    for (space, node, actor) in [(bad, bad_node.id, owner), (good, good_node.id, good_owner)] {
        files.soft_delete_node(space, node, actor, false).await?;
        sqlx::query(
            "UPDATE nodes SET purge_requested_at = now(), purge_after = now() WHERE id = $1",
        )
        .bind(node)
        .execute(&db.pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO audit_events(created_at, owner_user_id, actor_account_id, source, op_type, resource_type) \
         VALUES (now() - interval '181 days', $1, $1, 'system', 'test.expired', 'test')",
    ).bind(owner).execute(&db.pool).await?;
    sqlx::query("CREATE TABLE fail_node_purge(id uuid PRIMARY KEY)")
        .execute(&db.pool)
        .await?;
    sqlx::query("INSERT INTO fail_node_purge(id) VALUES ($1)")
        .bind(bad_node.id)
        .execute(&db.pool)
        .await?;
    sqlx::raw_sql(
        "CREATE FUNCTION fail_selected_node_purge() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF EXISTS(SELECT 1 FROM fail_node_purge WHERE id = OLD.id) THEN \
             RAISE EXCEPTION 'injected node purge failure'; END IF; RETURN OLD; END; $$; \
         CREATE TRIGGER fail_selected_node BEFORE DELETE ON nodes FOR EACH ROW EXECUTE FUNCTION fail_selected_node_purge();",
    ).execute(&db.pool).await?;
    assert!(PurgeRepo::new(db.pool.clone()).run_once().await.is_err());
    for (key, state, node) in [
        (&bad_file.object_key, "attached", Some(bad_node.id)),
        (&good_file.object_key, "delete_pending", None),
    ] {
        let ledger: (String, Option<Uuid>) = sqlx::query_as(
            "SELECT state, node_id FROM object_storage_objects WHERE object_key = $1",
        )
        .bind(key)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(ledger, (state.to_owned(), node));
    }
    let history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE op_type = 'test.expired'")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(history, 0);
    sqlx::query("DELETE FROM fail_node_purge")
        .execute(&db.pool)
        .await?;
    let retry = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(retry.nodes_deleted, 1);
    assert_eq!(
        retry.object_deletions_queued, 1,
        "committed peer intent is not queued again"
    );
    assert_eq!(retry.audit_events_deleted, 0);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_waits_for_bounded_ledger_and_connection_cleanup_without_resetting_retries()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = PURGE_TEST_MUTEX.lock().await;
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "purge-space-anchors").await?;
    let operation = Uuid::new_v4();
    sqlx::query(
        "UPDATE spaces SET deleted_at = now(), deleted_by_user_id = $2, purge_after = now(), \
             purge_requested_at = now(), deletion_operation_id = $3 WHERE id = $1",
    )
    .bind(space)
    .bind(owner)
    .bind(operation)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO object_storage_objects(id, object_key, space_id, parent_node_id, name, declared_byte_len, \
             media_type, state, retry_count, retry_after, last_error_code) \
         SELECT id, id::text, $1, $2, 'pending.bin', 1, 'application/octet-stream', 'delete_pending', 7, \
             now() + interval '1 hour', 'fixture' FROM (SELECT gen_random_uuid() AS id FROM generate_series(1, 201)) seed",
    ).bind(space).bind(root).execute(&db.pool).await?;
    sqlx::query("INSERT INTO accounts(kind) SELECT 'agent' FROM generate_series(1, 201)")
        .execute(&db.pool)
        .await?;
    sqlx::query("INSERT INTO agents(id, owner_user_id, name) SELECT id, $1, id::text FROM accounts WHERE kind = 'agent'")
        .bind(owner).execute(&db.pool).await?;
    sqlx::query(
        "INSERT INTO space_agent_connections(space_id, agent_id, permission, connected_by_user_id) \
         SELECT $1, id, 'read', $2 FROM agents WHERE owner_user_id = $2",
    )
    .bind(space)
    .bind(owner)
    .execute(&db.pool)
    .await?;
    for remaining in [101, 1, 0] {
        let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
        assert_eq!(run.spaces_deleted, u64::from(remaining == 0));
        assert_eq!(
            run.nodes_deleted, 0,
            "empty Space root is counted with its Space"
        );
        assert_eq!(
            run.object_deletions_queued, 0,
            "existing cleanup intent is not requeued"
        );
        let anchors: (i64, i64, bool) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM object_storage_objects WHERE space_id = $1), \
             (SELECT count(*) FROM space_agent_connections WHERE space_id = $1), \
             EXISTS(SELECT 1 FROM spaces WHERE id = $1)",
        )
        .bind(space)
        .fetch_one(&db.pool)
        .await?;
        assert_eq!(anchors, (remaining, remaining, remaining > 0));
        assert_eq!(run.resources_pending, remaining > 0);
    }
    let preserved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_storage_objects WHERE state = 'delete_pending' AND node_id IS NULL \
         AND parent_node_id IS NULL AND space_id IS NULL AND deletion_operation_id = $1 \
         AND retry_count = 7 AND retry_after > now() AND last_error_code = 'fixture'",
    ).bind(operation).fetch_one(&db.pool).await?;
    assert_eq!(preserved, 201);
    db.cleanup().await;
    Ok(())
}
