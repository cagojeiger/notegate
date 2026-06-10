//! Unified API-key persistence for user and agent accounts.

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result, limits};
use notegate_model::{ApiKey, ApiKeyCursor, CreateApiKey};
use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ApiKeyRepo {
    pool: PgPool,
    lookup_key_id: String,
    hash_version: i32,
}

impl ApiKeyRepo {
    #[cfg(any(test, feature = "test-util"))]
    pub fn new(pool: PgPool) -> Self {
        Self::with_lookup_key(pool, "test-lookup", 1)
    }

    pub fn with_lookup_key(
        pool: PgPool,
        lookup_key_id: impl Into<String>,
        hash_version: i32,
    ) -> Self {
        Self {
            pool,
            lookup_key_id: lookup_key_id.into(),
            hash_version,
        }
    }

    pub async fn list_by_account(
        &self,
        account_id: Uuid,
        limit: i64,
        cursor: Option<&ApiKeyCursor>,
    ) -> Result<Vec<ApiKey>> {
        let rows = match cursor {
            None => {
                sqlx::query_as::<_, ApiKeyRow>(&format!(
                    "SELECT {API_KEY_COLUMNS} FROM api_keys \
                     WHERE account_id = $1 AND revoked_at IS NULL \
                       AND expires_at > now() \
                     ORDER BY created_at DESC, id DESC LIMIT $2"
                ))
                .bind(account_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            Some(cursor) => {
                sqlx::query_as::<_, ApiKeyRow>(&format!(
                    "SELECT {API_KEY_COLUMNS} FROM api_keys \
                     WHERE account_id = $1 AND revoked_at IS NULL \
                       AND expires_at > now() \
                       AND (created_at, id) < ($2, $3) \
                     ORDER BY created_at DESC, id DESC LIMIT $4"
                ))
                .bind(account_id)
                .bind(cursor.created_at)
                .bind(cursor.id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(map_sqlx_error)?;
        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    pub async fn find_live_key(&self, account_id: Uuid, key_id: Uuid) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "SELECT {API_KEY_COLUMNS} FROM api_keys \
             WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL \
               AND expires_at > now()"
        ))
        .bind(key_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(ApiKey::from))
    }

    pub async fn find_live_account_id_by_key(
        &self,
        key_id: Uuid,
        token_hash: &str,
    ) -> Result<Option<Uuid>> {
        let account_id: Option<Uuid> = sqlx::query(
            "SELECT k.account_id FROM api_keys k \
             JOIN accounts acc ON acc.id = k.account_id \
             WHERE k.id = $1 AND k.token_hash = $2 \
               AND k.revoked_at IS NULL \
               AND k.expires_at > now() \
               AND acc.is_active = true \
               AND acc.deleted_at IS NULL",
        )
        .bind(key_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| row.get::<Uuid, _>("account_id"));

        if account_id.is_some()
            && let Err(error) = sqlx::query(
                "UPDATE api_keys SET last_used_at = now() \
                 WHERE id = $1 \
                   AND (last_used_at IS NULL OR last_used_at < now() - interval '1 hour')",
            )
            .bind(key_id)
            .execute(&self.pool)
            .await
        {
            tracing::warn!(event = "api_key.last_used_update_failed", %error);
        }

        Ok(account_id)
    }

    pub async fn count_live_keys(&self, account_id: Uuid) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM api_keys \
             WHERE account_id = $1 AND revoked_at IS NULL \
               AND expires_at > now()",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        usize::try_from(count).map_err(|_error| Error::internal("negative key count"))
    }

    /// Insert a key without enforcing the per-account live-key cap.
    ///
    /// Production code must use [`insert_key_with_cap`] or [`rotate_key`]. This
    /// helper exists for repository tests that need to seed exact key states.
    #[doc(hidden)]
    pub async fn insert_key_unchecked_for_test(&self, args: InsertApiKey<'_>) -> Result<ApiKey> {
        validate_command(args.command)?;
        let row = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "INSERT INTO api_keys \
             (id, account_id, token_prefix, token_hash, hash_key_id, hash_version, name, scopes, created_by, expires_at, rotated_from_key_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             RETURNING {API_KEY_COLUMNS}"
        ))
        .bind(args.key_id)
        .bind(args.account_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(&args.command.name)
        .bind(&args.command.scopes)
        .bind(args.created_by)
        .bind(args.command.expires_at)
        .bind(args.rotated_from_key_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(ApiKey::from(row))
    }

    /// Insert a new API key while enforcing the per-account live-key cap inside one
    /// transaction. Locks the account row (`SELECT id FROM accounts WHERE id=$1 FOR
    /// UPDATE`) so concurrent creates serialize on it, re-counts live keys inside the
    /// tx using the same predicate as [`count_live_keys`], and rejects with
    /// `Conflict` when the account is at the cap.
    ///
    /// Lock-order invariant: every transaction in this codebase that locks an
    /// `accounts` row AND writes `api_keys` takes the `accounts` row lock FIRST.
    /// `accounts[id]` is therefore the single serialization point for per-account
    /// key mutation, so no `accounts`<->`api_keys` lock cycle is constructible.
    pub async fn insert_key_with_cap(
        &self,
        args: InsertApiKey<'_>,
        max_live_keys: usize,
    ) -> Result<ApiKey> {
        validate_command(args.command)?;

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        // Lock the account row so concurrent creates serialize on it.
        let acct: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
                .bind(args.account_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if acct.is_none() {
            return Err(Error::not_found("account not found"));
        }

        // Re-count live keys INSIDE the tx using the same predicate as count_live_keys.
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM api_keys \
             WHERE account_id = $1 AND revoked_at IS NULL \
               AND expires_at > now()",
        )
        .bind(args.account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let live = usize::try_from(live).map_err(|_error| Error::internal("negative key count"))?;
        if live >= max_live_keys {
            return Err(Error::conflict(format!(
                "account already has the maximum of {max_live_keys} live API keys"
            )));
        }

        let row = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "INSERT INTO api_keys \
             (id, account_id, token_prefix, token_hash, hash_key_id, hash_version, name, scopes, created_by, expires_at, rotated_from_key_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             RETURNING {API_KEY_COLUMNS}"
        ))
        .bind(args.key_id)
        .bind(args.account_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(&args.command.name)
        .bind(&args.command.scopes)
        .bind(args.created_by)
        .bind(args.command.expires_at)
        .bind(args.rotated_from_key_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(ApiKey::from(row))
    }

    pub async fn rotate_key(
        &self,
        args: InsertApiKey<'_>,
        old_key_id: Uuid,
        revoked_by: Uuid,
        max_live_keys: usize,
    ) -> Result<ApiKey> {
        validate_command(args.command)?;

        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let old_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM api_keys \
             WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL \
               AND expires_at > now() \
             FOR UPDATE",
        )
        .bind(old_key_id)
        .bind(args.account_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if old_exists.is_none() {
            return Err(Error::not_found("api key not found"));
        }

        let live_without_old: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM api_keys \
             WHERE account_id = $1 AND id <> $2 AND revoked_at IS NULL \
               AND expires_at > now()",
        )
        .bind(args.account_id)
        .bind(old_key_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let live_without_old = usize::try_from(live_without_old)
            .map_err(|_error| Error::internal("negative key count"))?;
        if live_without_old >= max_live_keys {
            return Err(Error::conflict(format!(
                "account already has the maximum of {max_live_keys} live API keys"
            )));
        }

        let row = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "INSERT INTO api_keys \
             (id, account_id, token_prefix, token_hash, hash_key_id, hash_version, name, scopes, created_by, expires_at, rotated_from_key_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             RETURNING {API_KEY_COLUMNS}"
        ))
        .bind(args.key_id)
        .bind(args.account_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(&args.command.name)
        .bind(&args.command.scopes)
        .bind(args.created_by)
        .bind(args.command.expires_at)
        .bind(old_key_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE api_keys \
             SET revoked_at = now(), revoked_by = $3, revoked_reason = 'rotated' \
             WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL",
        )
        .bind(old_key_id)
        .bind(args.account_id)
        .bind(revoked_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(ApiKey::from(row))
    }

    pub async fn revoke_key(
        &self,
        account_id: Uuid,
        key_id: Uuid,
        revoked_by: Uuid,
        reason: Option<&str>,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE api_keys \
             SET revoked_at = now(), revoked_by = $3, revoked_reason = $4 \
             WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL",
        )
        .bind(key_id)
        .bind(account_id)
        .bind(revoked_by)
        .bind(reason)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("api key not found"));
        }
        Ok(())
    }
}

fn validate_command(command: &CreateApiKey) -> Result<()> {
    if command.name.trim().is_empty() {
        return Err(Error::validation("api key name cannot be empty"));
    }
    if command.name.chars().count() > limits::API_KEY_NAME_MAX_CHARS {
        return Err(Error::validation(format!(
            "api key name exceeds the maximum of {} characters",
            limits::API_KEY_NAME_MAX_CHARS
        )));
    }
    if !command.scopes.is_empty() {
        return Err(Error::validation("api key scopes must be empty"));
    }
    let Some(expires_at) = command.expires_at else {
        return Err(Error::validation("api key expires_at is required"));
    };
    if expires_at <= Utc::now() {
        return Err(Error::validation(
            "api key expires_at must be in the future",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct InsertApiKey<'a> {
    pub key_id: Uuid,
    pub account_id: Uuid,
    pub command: &'a CreateApiKey,
    pub token_prefix: &'a str,
    pub token_hash: &'a str,
    pub created_by: Uuid,
    pub rotated_from_key_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct ApiKeyRow {
    id: Uuid,
    account_id: Uuid,
    token_hash: String,
    name: String,
    scopes: Vec<String>,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_by: Option<Uuid>,
    revoked_reason: Option<String>,
    rotated_from_key_id: Option<Uuid>,
}

impl From<ApiKeyRow> for ApiKey {
    fn from(row: ApiKeyRow) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            token_hash: row.token_hash,
            name: row.name,
            scopes: row.scopes,
            created_by: row.created_by,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            revoked_by: row.revoked_by,
            revoked_reason: row.revoked_reason,
            rotated_from_key_id: row.rotated_from_key_id,
        }
    }
}

const API_KEY_COLUMNS: &str = "id, account_id, token_hash, name, scopes, created_by, created_at, \
     last_used_at, expires_at, revoked_at, revoked_by, revoked_reason, rotated_from_key_id";
