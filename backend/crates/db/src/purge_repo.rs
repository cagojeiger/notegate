//! Hard purge for soft-deleted spaces and nodes.
//!
//! Cross-process scheduling is owned by the reconciliation runtime. This repo
//! performs one bounded, atomic purge attempt.

use crate::map_sqlx_error;
use notegate_core::{Result, limits};
use sqlx::{PgPool, Row as _};

const SPACE_PURGE_BATCH: i64 = 100;
const NODE_PURGE_BATCH: i64 = 1_000;
const ACCOUNT_PURGE_BATCH: i64 = 100;
const API_KEY_PURGE_BATCH: i64 = 1_000;
const BROWSER_SESSION_PURGE_BATCH: i64 = 1_000;
const OBJECT_STORAGE_HISTORY_PURGE_BATCH: i64 = 1_000;
const AUDIT_EVENT_PURGE_BATCH: i64 = 1_000;
const FILE_CHANGE_EVENT_PURGE_BATCH: i64 = 1_000;
const MCP_INVOCATION_PURGE_BATCH: i64 = 1_000;

#[derive(Debug, Clone)]
pub struct PurgeRepo {
    pool: PgPool,
}

impl PurgeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run one bounded purge attempt in a single transaction.
    pub async fn run_once(&self) -> Result<PurgeRun> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        // Safety net for requests missed during soft delete: queue physical
        // object deletion before semantic rows disappear. The operational
        // ledger survives the following cascades and is processed outside this
        // transaction by object-storage cleanup reconciliation.
        let queued_for_spaces = sqlx::query(
            "UPDATE object_storage_objects f SET \
                 state = 'delete_pending', \
                 delete_requested_at = COALESCE(delete_requested_at, now()), \
                 retry_after = NULL, last_error_code = NULL \
             WHERE f.state = 'attached' AND f.space_id IN ( \
                 SELECT id FROM spaces \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id LIMIT $1 \
             )",
        )
        .bind(SPACE_PURGE_BATCH)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();

        let queued_for_nodes = sqlx::query(
            "WITH RECURSIVE due_roots AS ( \
                 SELECT id FROM nodes \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id LIMIT $1 \
             ), due_nodes AS ( \
                 SELECT id FROM due_roots \
                 UNION \
                 SELECT child.id FROM nodes child \
                 JOIN due_nodes parent ON child.parent_id = parent.id \
             ) \
             UPDATE object_storage_objects f SET \
                 state = 'delete_pending', \
                 delete_requested_at = COALESCE(delete_requested_at, now()), \
                 retry_after = NULL, last_error_code = NULL \
             WHERE f.state = 'attached' AND f.node_id IN (SELECT id FROM due_nodes)",
        )
        .bind(NODE_PURGE_BATCH)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected();

        // Space hard delete cascades agent connections, nodes, text objects, and file objects.
        let spaces_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM spaces \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id \
                 LIMIT $1 \
             ), deleted AS ( \
                 DELETE FROM spaces w USING due \
                 WHERE w.id = due.id \
                 RETURNING w.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(SPACE_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        // Node hard delete cascades text/file objects and any descendant nodes. The CTE
        // limits the number of selected due nodes; cascaded descendants may make
        // the physical row count larger, which is acceptable and bounded by the
        // product subtree/space limits.
        let nodes_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM nodes \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id \
                 LIMIT $1 \
             ), deleted AS ( \
                 DELETE FROM nodes n USING due \
                 WHERE n.id = due.id \
                 RETURNING n.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(NODE_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        // ADR 0004: anonymize soft-deleted accounts whose retention window has elapsed.
        // Wipe PII and free the `provider_sub_hash` tombstone, but KEEP the (now
        // identifier-less) account/user rows for attribution. Freeing the tombstone lets
        // the same OAuth sub register fresh on a later login.
        let accounts_anonymized: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT a.id FROM accounts a \
                 JOIN users u ON u.id = a.id \
                 WHERE a.kind = 'user' AND a.deleted_at IS NOT NULL \
                   AND a.deleted_at + make_interval(days => $1::int) <= now() \
                   AND u.anonymized_at IS NULL \
                 ORDER BY a.deleted_at, a.id \
                 LIMIT $2 \
             ), anon_accounts AS ( \
                 UPDATE accounts SET \
                     display_name_ciphertext = NULL, display_name_nonce = NULL, \
                     display_name_enc_key_id = NULL, display_name_enc_version = NULL, \
                     updated_at = now() \
                 FROM due WHERE accounts.id = due.id \
                 RETURNING accounts.id \
             ), anon_users AS ( \
                 UPDATE users SET \
                     provider_sub_hash = NULL, provider_sub_hash_key_id = NULL, \
                     provider_sub_hash_version = NULL, email_ciphertext = NULL, \
                     email_nonce = NULL, email_enc_key_id = NULL, email_enc_version = NULL, \
                     email_hash = NULL, email_hash_key_id = NULL, email_hash_version = NULL, \
                     anonymized_at = now() \
                 FROM due WHERE users.id = due.id \
                 RETURNING users.id \
             ) \
             SELECT count(*) AS anonymized_count FROM anon_users",
        )
        .bind(i32::try_from(limits::ACCOUNT_DELETION_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(ACCOUNT_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("anonymized_count");

        // Hard delete API keys that have been dead (revoked or expired) for longer
        // than the retention window. A key dies at the earlier of its revoke time and
        // its expiry; never-revoked keys die at `expires_at`. The live-key listing and
        // the per-account cap already ignore dead keys, so this only reclaims storage
        // after a short audit window.
        let api_keys_deleted: i64 = sqlx::query(
            "WITH dead AS ( \
                 SELECT id, LEAST(COALESCE(revoked_at, expires_at), expires_at) AS dead_at \
                 FROM api_keys \
                 WHERE revoked_at IS NOT NULL OR expires_at <= now() \
             ), due AS ( \
                 SELECT id FROM dead \
                 WHERE dead_at + make_interval(days => $1::int) <= now() \
                 ORDER BY dead_at, id \
                 LIMIT $2 \
             ), deleted AS ( \
                 DELETE FROM api_keys k USING due \
                 WHERE k.id = due.id \
                 RETURNING k.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::DEAD_API_KEY_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(API_KEY_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        let browser_sessions_deleted: i64 = sqlx::query(
            "WITH dead AS ( \
                 SELECT id, LEAST(COALESCE(revoked_at, expires_at), expires_at) AS dead_at \
                 FROM browser_sessions \
                 WHERE revoked_at IS NOT NULL OR expires_at <= now() \
             ), due AS ( \
                 SELECT id FROM dead \
                 WHERE dead_at + make_interval(days => $1::int) <= now() \
                 ORDER BY dead_at, id \
                 LIMIT $2 \
             ), deleted AS ( \
                 DELETE FROM browser_sessions s USING due \
                 WHERE s.id = due.id \
                 RETURNING s.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::DEAD_API_KEY_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(BROWSER_SESSION_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        let object_storage_history_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM object_storage_objects \
                 WHERE state IN ('expired','deleted') \
                   AND COALESCE(deleted_at, last_activity_at) \
                       <= now() - make_interval(days => $1::int) \
                 ORDER BY COALESCE(deleted_at, last_activity_at), id \
                 LIMIT $2 \
                 FOR UPDATE SKIP LOCKED \
             ), deleted AS ( \
                 DELETE FROM object_storage_objects o USING due \
                 WHERE o.id = due.id \
                 RETURNING o.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::OBJECT_STORAGE_HISTORY_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(OBJECT_STORAGE_HISTORY_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        let audit_events_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM audit_events \
                 WHERE created_at <= now() - make_interval(days => $1::int) \
                 ORDER BY created_at, id \
                 LIMIT $2 \
             ), deleted AS ( \
                 DELETE FROM audit_events e USING due \
                 WHERE e.id = due.id \
                 RETURNING e.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::AUDIT_EVENT_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(AUDIT_EVENT_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        let file_change_events_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM file_change_events \
                 WHERE created_at <= now() - make_interval(days => $1::int) \
                 ORDER BY created_at, id \
                 LIMIT $2 \
             ), deleted AS ( \
                 DELETE FROM file_change_events e USING due \
                 WHERE e.id = due.id \
                 RETURNING e.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::FILE_CHANGE_EVENT_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(FILE_CHANGE_EVENT_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        let mcp_invocations_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM mcp_invocations \
                 WHERE created_at <= now() - make_interval(days => $1::int) \
                 ORDER BY created_at, id \
                 LIMIT $2 \
             ), deleted AS ( \
                 DELETE FROM mcp_invocations m USING due \
                 WHERE m.id = due.id \
                 RETURNING m.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(i32::try_from(limits::MCP_INVOCATION_RETENTION_DAYS).unwrap_or(i32::MAX))
        .bind(MCP_INVOCATION_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(PurgeRun {
            spaces_deleted: spaces_deleted.max(0) as u64,
            nodes_deleted: nodes_deleted.max(0) as u64,
            accounts_anonymized: accounts_anonymized.max(0) as u64,
            api_keys_deleted: api_keys_deleted.max(0) as u64,
            browser_sessions_deleted: browser_sessions_deleted.max(0) as u64,
            object_storage_history_deleted: object_storage_history_deleted.max(0) as u64,
            audit_events_deleted: audit_events_deleted.max(0) as u64,
            file_change_events_deleted: file_change_events_deleted.max(0) as u64,
            mcp_invocations_deleted: mcp_invocations_deleted.max(0) as u64,
            object_deletions_queued: queued_for_spaces + queued_for_nodes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRun {
    pub spaces_deleted: u64,
    pub nodes_deleted: u64,
    pub accounts_anonymized: u64,
    pub api_keys_deleted: u64,
    pub browser_sessions_deleted: u64,
    pub object_storage_history_deleted: u64,
    pub audit_events_deleted: u64,
    pub file_change_events_deleted: u64,
    pub mcp_invocations_deleted: u64,
    pub object_deletions_queued: u64,
}
