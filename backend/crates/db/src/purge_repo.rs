//! Hard purge for soft-deleted workspaces and nodes.
//!
//! The purge run is protected by a Postgres advisory transaction lock. Multiple
//! application processes may start this worker, but only one process can execute
//! a purge transaction at a time for a given database.

use crate::map_sqlx_error;
use notegate_core::{Result, limits};
use sqlx::{PgPool, Row as _};

/// Stable advisory lock key for notegate purge runs.
///
/// This is an arbitrary signed 64-bit namespace value. It must stay stable so
/// all notegate instances contend on the same database lock.
const PURGE_ADVISORY_LOCK_KEY: i64 = 0x4e47_5055_5247_4501;
const WORKSPACE_PURGE_BATCH: i64 = 100;
const NODE_PURGE_BATCH: i64 = 1_000;
const ACCOUNT_PURGE_BATCH: i64 = 100;

#[derive(Debug, Clone)]
pub struct PurgeRepo {
    pool: PgPool,
}

impl PurgeRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run one bounded purge attempt.
    ///
    /// Returns immediately with `lock_acquired=false` if another notegate
    /// process is already purging this database.
    pub async fn run_once(&self) -> Result<PurgeRun> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let lock_acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
            .bind(PURGE_ADVISORY_LOCK_KEY)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        if !lock_acquired {
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(PurgeRun {
                lock_acquired: false,
                workspaces_deleted: 0,
                nodes_deleted: 0,
                accounts_anonymized: 0,
            });
        }

        // Workspace hard delete cascades workspace_access, nodes, and documents.
        let workspaces_deleted: i64 = sqlx::query(
            "WITH due AS ( \
                 SELECT id FROM workspaces \
                 WHERE deleted_at IS NOT NULL AND purge_after <= now() \
                 ORDER BY purge_after, id \
                 LIMIT $1 \
             ), deleted AS ( \
                 DELETE FROM workspaces w USING due \
                 WHERE w.id = due.id \
                 RETURNING w.id \
             ) \
             SELECT count(*) AS deleted_count FROM deleted",
        )
        .bind(WORKSPACE_PURGE_BATCH)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?
        .get("deleted_count");

        // Node hard delete cascades documents and any descendant nodes. The CTE
        // limits the number of selected due nodes; cascaded descendants may make
        // the physical row count larger, which is acceptable and bounded by the
        // product subtree/workspace limits.
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

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(PurgeRun {
            lock_acquired: true,
            workspaces_deleted: workspaces_deleted.max(0) as u64,
            nodes_deleted: nodes_deleted.max(0) as u64,
            accounts_anonymized: accounts_anonymized.max(0) as u64,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRun {
    pub lock_acquired: bool,
    pub workspaces_deleted: u64,
    pub nodes_deleted: u64,
    pub accounts_anonymized: u64,
}
