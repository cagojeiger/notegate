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

    pub async fn insert_session(
        &self,
        args: InsertBrowserSession<'_>,
        issuer: &str,
    ) -> Result<BrowserSession> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let row = sqlx::query_as::<_, BrowserSessionRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO browser_sessions \
             (id, user_id, token_prefix, token_hash, hash_key_id, hash_version, \
              idle_expires_at, expires_at, oidc_issuer, client_environment) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             RETURNING {BROWSER_SESSION_RETURNING_COLUMNS}"
        )))
        .bind(args.session_id)
        .bind(args.user_id)
        .bind(args.token_prefix)
        .bind(args.token_hash)
        .bind(&self.lookup_key_id)
        .bind(self.hash_version)
        .bind(args.idle_expires_at)
        .bind(args.expires_at)
        .bind(issuer)
        .bind(args.client_environment)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        audit_events::session_logged_in(
            &mut tx,
            args.user_id,
            args.session_id,
            args.client_environment,
        )
        .await?;
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
                    bs.idle_expires_at AS session_idle_expires_at, bs.oidc_issuer, bs.created_at AS session_created_at, \
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
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM browser_sessions WHERE id=$1")
                .bind(session_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if let Some(owner) = owner {
            sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
                .bind(owner)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        }
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

    pub async fn renew_idle_expiry(
        &self,
        session_id: Uuid,
        token_hash: &str,
        issuer: &str,
    ) -> Result<bool> {
        let active = active_account_predicate("a.");
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE browser_sessions bs \
             SET idle_expires_at = LEAST(bs.expires_at, GREATEST(bs.idle_expires_at, \
                     clock_timestamp() + make_interval(days => $4::int))), \
                 last_used_at = clock_timestamp(), updated_at = clock_timestamp() \
             FROM accounts a JOIN users u ON u.id = a.id \
             WHERE bs.id = $1 AND bs.token_hash = $2 AND bs.oidc_issuer = $3 \
               AND bs.user_id = a.id AND a.kind = 'user' AND {active} \
               AND bs.revoked_at IS NULL AND bs.expires_at > clock_timestamp() \
               AND bs.idle_expires_at > clock_timestamp()"
        )))
        .bind(session_id)
        .bind(token_hash)
        .bind(issuer)
        .bind(notegate_core::limits::BROWSER_SESSION_IDLE_TTL_DAYS)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn list_owned_live(
        &self,
        user_id: Uuid,
        issuer: &str,
        current_session_id: Uuid,
    ) -> Result<Vec<BrowserSessionSummary>> {
        let live = live_browser_session_predicate("");
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id, client_environment, created_at, last_used_at, idle_expires_at, expires_at \
             FROM browser_sessions WHERE user_id=$1 AND oidc_issuer=$2 AND {live} \
             ORDER BY (id=$3) DESC, created_at DESC, id DESC LIMIT 100"
        )))
        .bind(user_id)
        .bind(issuer)
        .bind(current_session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    pub async fn revoke_owned_sessions(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
        except_session_id: Option<Uuid>,
    ) -> Result<u64> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "WITH targets AS (SELECT id FROM browser_sessions \
             WHERE user_id=$1 AND revoked_at IS NULL \
               AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR id<>$3) \
             ORDER BY id FOR UPDATE) \
             UPDATE browser_sessions bs SET revoked_at=now(), revoked_reason='user_revoked', updated_at=now() \
             FROM targets WHERE bs.id=targets.id RETURNING bs.id"
        )
        .bind(user_id).bind(session_id).bind(except_session_id)
        .fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
        for id in &ids {
            audit_events::session_revoked(&mut tx, user_id, *id).await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(ids.len() as u64)
    }
}

#[derive(Debug, FromRow)]
pub struct BrowserSessionSummary {
    pub id: Uuid,
    pub client_environment: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub idle_expires_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BrowserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub oidc_issuer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct InsertBrowserSession<'a> {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub token_prefix: &'a str,
    pub token_hash: &'a str,
    pub client_environment: Option<&'a str>,
    pub idle_expires_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct LiveBrowserSessionCaller {
    pub session_id: Uuid,
    pub oidc_issuer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub account: Account,
    pub user: User,
}

#[derive(Debug, FromRow)]
struct LiveBrowserSessionCallerRow {
    session_id: Uuid,
    session_idle_expires_at: DateTime<Utc>,
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
            idle_expires_at: self.session_idle_expires_at,
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
    oidc_issuer: Option<String>,
    created_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<BrowserSessionRow> for BrowserSession {
    fn from(row: BrowserSessionRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            idle_expires_at: row.idle_expires_at,
            oidc_issuer: row.oidc_issuer,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}

fn live_browser_session_predicate(prefix: &str) -> String {
    format!(
        "{prefix}revoked_at IS NULL AND {prefix}expires_at > now() \
         AND {prefix}idle_expires_at > now()"
    )
}

const BROWSER_SESSION_RETURNING_COLUMNS: &str = "id, user_id, \
     idle_expires_at, expires_at, oidc_issuer, created_at";

const BROWSER_SESSION_SELECT_COLUMNS: &str = "bs.id, bs.user_id, \
     bs.idle_expires_at, bs.expires_at, bs.oidc_issuer, bs.created_at";

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
