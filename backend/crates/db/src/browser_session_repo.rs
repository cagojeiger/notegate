//! NoteGate-owned browser sessions and transactional login/logout audit records.

use crate::account_repo::{USER_CALLER_COLUMNS, UserCallerRow};
use crate::{active_account_predicate, audit_events, map_sqlx_error};
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_core::security::PiiCrypto;
use notegate_model::{Account, User};
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

    pub async fn insert_local_session(
        &self,
        args: InsertBrowserSession<'_>,
        issuer: &str,
    ) -> Result<BrowserSession> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let row = sqlx::query_as::<_, BrowserSessionRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO browser_sessions \
             (id, user_id, token_prefix, token_hash, hash_key_id, hash_version, \
              validated_until, expires_at, local_session, oidc_issuer) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             RETURNING {BROWSER_SESSION_RETURNING_COLUMNS}"
        )))
        .bind(args.session_id)
        .bind(args.user_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(args.validated_until)
        .bind(args.expires_at)
        .bind(true)
        .bind(issuer)
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
        let row = sqlx::query_as::<_, BrowserSessionRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {BROWSER_SESSION_SELECT_COLUMNS} \
             FROM browser_sessions bs \
             JOIN accounts acc ON acc.id = bs.user_id \
             WHERE bs.id = $1 AND bs.token_hash = $2 \
               AND {live} \
               AND {active}"
        )))
        .bind(session_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(BrowserSession::from))
    }

    /// Resolve a live browser session and its user caller in one database query.
    /// No external refresh credential is read on the authentication path.
    pub async fn find_live_caller_by_token(
        &self,
        session_id: Uuid,
        token_hash: &str,
        crypto: &PiiCrypto,
    ) -> Result<Option<LiveBrowserSessionCaller>> {
        let live = live_browser_session_predicate("bs.");
        let active = active_account_predicate("a.");
        let row = sqlx::query_as::<_, LiveBrowserSessionCallerRow>(sqlx::AssertSqlSafe(format!(
            "SELECT bs.id AS session_id, \
                    bs.validated_until AS session_validated_until, bs.local_session, bs.oidc_issuer, bs.created_at AS session_created_at, \
                    {USER_CALLER_COLUMNS} \
             FROM browser_sessions bs \
             JOIN accounts a ON a.id = bs.user_id \
             JOIN users u ON u.id = a.id \
             WHERE bs.id = $1 AND bs.token_hash = $2 \
               AND {live} \
               AND {active} \
               AND a.kind = 'user'"
        )))
        .bind(session_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        row.map(|row| row.into_caller(crypto)).transpose()
    }

    pub async fn find_by_token(
        &self,
        session_id: Uuid,
        token_hash: &str,
    ) -> Result<Option<BrowserSession>> {
        let row = sqlx::query_as::<_, BrowserSessionRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {BROWSER_SESSION_SELECT_COLUMNS} \
             FROM browser_sessions bs \
             WHERE bs.id = $1 AND bs.token_hash = $2"
        )))
        .bind(session_id)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(row.map(BrowserSession::from))
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
}

#[derive(Debug, Clone)]
pub struct BrowserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub local_session: bool,
    pub oidc_issuer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub validated_until: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct InsertBrowserSession<'a> {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub token_prefix: &'a str,
    pub token_hash: &'a str,
    pub validated_until: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct LiveBrowserSessionCaller {
    pub session_id: Uuid,
    pub local_session: bool,
    pub oidc_issuer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub validated_until: DateTime<Utc>,
    pub account: Account,
    pub user: User,
}

#[derive(Debug, FromRow)]
struct LiveBrowserSessionCallerRow {
    session_id: Uuid,
    session_validated_until: DateTime<Utc>,
    local_session: bool,
    oidc_issuer: Option<String>,
    session_created_at: DateTime<Utc>,
    #[sqlx(flatten)]
    caller: UserCallerRow,
}

impl LiveBrowserSessionCallerRow {
    fn into_caller(self, crypto: &PiiCrypto) -> Result<LiveBrowserSessionCaller> {
        let (account, user) = self.caller.into_parts(crypto)?;
        Ok(LiveBrowserSessionCaller {
            session_id: self.session_id,
            validated_until: self.session_validated_until,
            local_session: self.local_session,
            oidc_issuer: self.oidc_issuer,
            created_at: self.session_created_at,
            account,
            user,
        })
    }
}

#[derive(Debug, FromRow)]
struct BrowserSessionRow {
    id: Uuid,
    user_id: Uuid,
    local_session: bool,
    oidc_issuer: Option<String>,
    created_at: DateTime<Utc>,
    validated_until: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<BrowserSessionRow> for BrowserSession {
    fn from(row: BrowserSessionRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            validated_until: row.validated_until,
            local_session: row.local_session,
            oidc_issuer: row.oidc_issuer,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}

fn live_browser_session_predicate(prefix: &str) -> String {
    format!("{prefix}revoked_at IS NULL AND {prefix}expires_at > now()")
}

const BROWSER_SESSION_RETURNING_COLUMNS: &str = "id, user_id, \
     validated_until, expires_at, local_session, oidc_issuer, created_at";

const BROWSER_SESSION_SELECT_COLUMNS: &str = "bs.id, bs.user_id, \
     bs.validated_until, bs.expires_at, bs.local_session, bs.oidc_issuer, bs.created_at";

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
