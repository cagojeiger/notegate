//! Browser session persistence for server-side OAuth refresh.

use crate::{active_account_predicate, audit_events, map_sqlx_error};
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_core::security::EncryptedField;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BrowserSessionRepo {
    pool: PgPool,
    lookup_key_id: String,
    hash_version: i32,
}

impl BrowserSessionRepo {
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

    pub async fn insert_session(&self, args: InsertBrowserSession<'_>) -> Result<BrowserSession> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let row = sqlx::query_as::<_, BrowserSessionRow>(&format!(
            "INSERT INTO browser_sessions \
             (id, user_id, token_prefix, token_hash, hash_key_id, hash_version, \
              refresh_token_ciphertext, refresh_token_nonce, refresh_token_enc_key_id, refresh_token_enc_version, \
              validated_until, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             RETURNING {BROWSER_SESSION_RETURNING_COLUMNS}"
        ))
        .bind(args.session_id)
        .bind(args.user_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(&args.refresh_token.ciphertext)
        .bind(&args.refresh_token.nonce)
        .bind(args.refresh_token_enc_key_id)
        .bind(args.refresh_token_enc_version)
        .bind(args.validated_until)
        .bind(args.expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        audit_events::session_logged_in(&mut tx, args.user_id, args.session_id).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(BrowserSession::from(row))
    }

    pub async fn find_live_by_token(
        &self,
        session_id: Uuid,
        token_hash: &str,
    ) -> Result<Option<BrowserSession>> {
        let live = live_browser_session_predicate("bs.");
        let active = active_account_predicate("acc.");
        let row = sqlx::query_as::<_, BrowserSessionRow>(&format!(
            "SELECT {BROWSER_SESSION_SELECT_COLUMNS} \
             FROM browser_sessions bs \
             JOIN accounts acc ON acc.id = bs.user_id \
             WHERE bs.id = $1 AND bs.token_hash = $2 \
               AND {live} \
               AND {active}"
        ))
        .bind(session_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(BrowserSession::from))
    }

    pub async fn find_by_token(
        &self,
        session_id: Uuid,
        token_hash: &str,
    ) -> Result<Option<BrowserSession>> {
        let row = sqlx::query_as::<_, BrowserSessionRow>(&format!(
            "SELECT {BROWSER_SESSION_SELECT_COLUMNS} \
             FROM browser_sessions bs \
             WHERE bs.id = $1 AND bs.token_hash = $2"
        ))
        .bind(session_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(BrowserSession::from))
    }

    pub async fn claim_refresh(
        &self,
        session_id: Uuid,
        token_hash: &str,
        refresh_claim_id: Uuid,
    ) -> Result<Option<BrowserSession>> {
        let live = live_browser_session_predicate("bs.");
        let active = active_account_predicate("acc.");
        // Longer than the HTTP client timeout so normal refresh requests do not overlap,
        // but a stuck worker can be retried without waiting for the session max TTL.
        let row = sqlx::query_as::<_, BrowserSessionRow>(&format!(
            "UPDATE browser_sessions AS bs \
             SET refresh_started_at = now(), refresh_claim_id = $3, updated_at = now() \
             FROM accounts acc \
             WHERE acc.id = bs.user_id \
               AND bs.id = $1 AND bs.token_hash = $2 \
               AND bs.validated_until <= now() \
               AND (bs.refresh_started_at IS NULL OR bs.refresh_started_at < now() - interval '30 seconds') \
               AND {live} \
               AND {active} \
             RETURNING {BROWSER_SESSION_SELECT_COLUMNS}"
        ))
        .bind(session_id)
        .bind(token_hash)
        .bind(refresh_claim_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(BrowserSession::from))
    }

    pub async fn touch_last_used(&self, session_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE browser_sessions SET last_used_at = now(), updated_at = now() \
             WHERE id = $1 \
               AND (last_used_at IS NULL OR last_used_at < now() - interval '1 hour')",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn mark_refreshed(
        &self,
        session_id: Uuid,
        refresh_claim_id: Uuid,
        validated_until: DateTime<Utc>,
        rotated_refresh_token: Option<RotatedRefreshToken<'_>>,
    ) -> Result<bool> {
        let result = match rotated_refresh_token {
            Some(rotated) => sqlx::query(
                "UPDATE browser_sessions \
                     SET refresh_token_ciphertext = $2, refresh_token_nonce = $3, \
                         refresh_token_enc_key_id = $4, refresh_token_enc_version = $5, \
                         validated_until = $6, last_refreshed_at = now(), last_used_at = now(), \
                         refresh_started_at = NULL, refresh_claim_id = NULL, updated_at = now() \
                     WHERE id = $1 AND refresh_claim_id = $7 \
                       AND revoked_at IS NULL AND expires_at > now()",
            )
            .bind(session_id)
            .bind(&rotated.refresh_token.ciphertext)
            .bind(&rotated.refresh_token.nonce)
            .bind(rotated.refresh_token_enc_key_id)
            .bind(rotated.refresh_token_enc_version)
            .bind(validated_until)
            .bind(refresh_claim_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?,
            None => sqlx::query(
                "UPDATE browser_sessions \
                     SET validated_until = $2, last_refreshed_at = now(), last_used_at = now(), \
                         refresh_started_at = NULL, refresh_claim_id = NULL, updated_at = now() \
                     WHERE id = $1 AND refresh_claim_id = $3 \
                       AND revoked_at IS NULL AND expires_at > now()",
            )
            .bind(session_id)
            .bind(validated_until)
            .bind(refresh_claim_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?,
        };
        Ok(result.rows_affected() == 1)
    }

    pub async fn store_rotated_refresh_token_and_clear_claim(
        &self,
        session_id: Uuid,
        refresh_claim_id: Uuid,
        rotated_refresh_token: RotatedRefreshToken<'_>,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE browser_sessions \
                 SET refresh_token_ciphertext = $3, refresh_token_nonce = $4, \
                     refresh_token_enc_key_id = $5, refresh_token_enc_version = $6, \
                     refresh_started_at = NULL, refresh_claim_id = NULL, updated_at = now() \
                 WHERE id = $1 AND refresh_claim_id = $2 \
                   AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(session_id)
        .bind(refresh_claim_id)
        .bind(&rotated_refresh_token.refresh_token.ciphertext)
        .bind(&rotated_refresh_token.refresh_token.nonce)
        .bind(rotated_refresh_token.refresh_token_enc_key_id)
        .bind(rotated_refresh_token.refresh_token_enc_version)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn revoke_session_for_logout(&self, session_id: Uuid) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE browser_sessions \
             SET revoked_at = now(), revoked_reason = 'logout', updated_at = now() \
             WHERE id = $1 AND revoked_at IS NULL \
             RETURNING user_id",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if let Some(user_id) = user_id {
            audit_events::session_logged_out(&mut tx, user_id, session_id).await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(user_id.is_some())
    }

    pub async fn revoke_claimed_refresh(
        &self,
        session_id: Uuid,
        refresh_claim_id: Uuid,
        reason: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE browser_sessions \
             SET revoked_at = now(), revoked_reason = $3, \
                 refresh_started_at = NULL, refresh_claim_id = NULL, updated_at = now() \
             WHERE id = $1 AND refresh_claim_id = $2 AND revoked_at IS NULL \
             RETURNING user_id",
        )
        .bind(session_id)
        .bind(refresh_claim_id)
        .bind(reason)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if let Some(user_id) = user_id {
            audit_events::session_revoked(&mut tx, user_id, session_id, reason).await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(user_id.is_some())
    }

    pub async fn clear_refresh_claim(
        &self,
        session_id: Uuid,
        refresh_claim_id: Uuid,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE browser_sessions \
             SET refresh_started_at = NULL, refresh_claim_id = NULL, updated_at = now() \
             WHERE id = $1 AND refresh_claim_id = $2 AND revoked_at IS NULL",
        )
        .bind(session_id)
        .bind(refresh_claim_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(Debug, Clone)]
pub struct BrowserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token: EncryptedField,
    pub refresh_token_enc_key_id: String,
    pub refresh_token_enc_version: i32,
    pub validated_until: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct InsertBrowserSession<'a> {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub token_prefix: &'a str,
    pub token_hash: &'a str,
    pub refresh_token: &'a EncryptedField,
    pub refresh_token_enc_key_id: &'a str,
    pub refresh_token_enc_version: i32,
    pub validated_until: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct RotatedRefreshToken<'a> {
    pub refresh_token: &'a EncryptedField,
    pub refresh_token_enc_key_id: &'a str,
    pub refresh_token_enc_version: i32,
}

#[derive(Debug, FromRow)]
struct BrowserSessionRow {
    id: Uuid,
    user_id: Uuid,
    refresh_token_ciphertext: Vec<u8>,
    refresh_token_nonce: Vec<u8>,
    refresh_token_enc_key_id: String,
    refresh_token_enc_version: i32,
    validated_until: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<BrowserSessionRow> for BrowserSession {
    fn from(row: BrowserSessionRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            refresh_token: EncryptedField {
                ciphertext: row.refresh_token_ciphertext,
                nonce: row.refresh_token_nonce,
            },
            refresh_token_enc_key_id: row.refresh_token_enc_key_id,
            refresh_token_enc_version: row.refresh_token_enc_version,
            validated_until: row.validated_until,
            expires_at: row.expires_at,
        }
    }
}

fn live_browser_session_predicate(prefix: &str) -> String {
    format!("{prefix}revoked_at IS NULL AND {prefix}expires_at > now()")
}

const BROWSER_SESSION_RETURNING_COLUMNS: &str = "id, user_id, refresh_token_ciphertext, \
     refresh_token_nonce, refresh_token_enc_key_id, refresh_token_enc_version, \
     validated_until, expires_at";

const BROWSER_SESSION_SELECT_COLUMNS: &str = "bs.id, bs.user_id, bs.refresh_token_ciphertext, \
     bs.refresh_token_nonce, bs.refresh_token_enc_key_id, bs.refresh_token_enc_version, \
     bs.validated_until, bs.expires_at";

pub fn token_prefix(session_id: Uuid) -> String {
    format!("ngs_v1_{session_id}")
}

pub fn format_token(session_id: Uuid, secret: &str) -> String {
    format!("{}_{}", token_prefix(session_id), secret)
}

pub fn parse_token(token: &str) -> Option<(Uuid, &str)> {
    let rest = token.strip_prefix("ngs_v1_")?;
    let (session_id, secret) = rest.split_once('_')?;
    if secret.is_empty() {
        return None;
    }
    let session_id = Uuid::parse_str(session_id).ok()?;
    Some((session_id, secret))
}
