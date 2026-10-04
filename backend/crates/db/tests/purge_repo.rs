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
         VALUES (now() - interval '366 days', $1, $1, 'system', 'test.expired', 'test')",
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
