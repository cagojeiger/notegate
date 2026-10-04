//! Anonymize deleted accounts and reclaim expired credentials and sessions.

use crate::map_sqlx_error;
use notegate_core::{Result, limits};
use sqlx::{PgPool, Row as _};

const ACCOUNT_PURGE_BATCH: i64 = 100;
const API_KEY_PURGE_BATCH: i64 = 1_000;
const BROWSER_SESSION_PURGE_BATCH: i64 = 1_000;

pub(super) struct PurgedIdentities {
    pub(super) accounts_anonymized: i64,
    pub(super) api_keys_deleted: i64,
    pub(super) browser_sessions_deleted: i64,
}

pub(super) async fn purge(pool: &PgPool) -> Result<PurgedIdentities> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

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

    tx.commit().await.map_err(map_sqlx_error)?;
    tracing::info!(
        event = "purge.group_completed",
        group = "identities",
        accounts_anonymized,
        api_keys_deleted,
        browser_sessions_deleted,
    );

    Ok(PurgedIdentities {
        accounts_anonymized,
        api_keys_deleted,
        browser_sessions_deleted,
    })
}
