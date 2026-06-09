//! Unified API-key persistence for user and agent accounts.

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::{ApiKey, CreateApiKey};
use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ApiKeyRepo {
    pool: PgPool,
    lookup_key_id: String,
    hash_version: i32,
}

impl ApiKeyRepo {
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

    pub async fn list_by_account(&self, account_id: Uuid) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "SELECT {API_KEY_COLUMNS} FROM api_keys \
             WHERE account_id = $1 ORDER BY created_at DESC, id DESC"
        ))
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    pub async fn find_live_key(&self, account_id: Uuid, key_id: Uuid) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKeyRow>(&format!(
            "SELECT {API_KEY_COLUMNS} FROM api_keys \
             WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL \
               AND (expires_at IS NULL OR expires_at > now())"
        ))
        .bind(key_id)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(ApiKey::from))
    }

    pub async fn find_live_account_id_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Uuid>> {
        let account_id: Option<Uuid> = sqlx::query(
            "SELECT k.account_id FROM api_keys k \
             JOIN accounts acc ON acc.id = k.account_id \
             WHERE k.token_hash = $1 \
               AND k.revoked_at IS NULL \
               AND (k.expires_at IS NULL OR k.expires_at > now()) \
               AND acc.is_active = true \
               AND acc.deleted_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(|row| row.get::<Uuid, _>("account_id"));

        if account_id.is_some()
            && let Err(error) =
                sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE token_hash = $1")
                    .bind(token_hash)
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
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        usize::try_from(count).map_err(|_error| Error::internal("negative key count"))
    }

    pub async fn insert_key(&self, args: InsertApiKey<'_>) -> Result<ApiKey> {
        if !args.command.scopes.is_empty() {
            return Err(Error::validation("api key scopes must be empty"));
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
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
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
    expires_at: Option<DateTime<Utc>>,
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
