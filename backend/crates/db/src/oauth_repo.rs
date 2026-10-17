//! Local OAuth persistence. One-use credentials and revocation are transaction boundaries.
#![allow(clippy::too_many_arguments)]

use chrono::{DateTime, Utc};
use notegate_core::Result;
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Clone, Debug)]
pub struct OAuthRepo {
    pool: PgPool,
}

#[derive(Debug, FromRow)]
pub struct Authorization {
    pub id: Uuid,
    pub client_id: String,
    pub client_name: String,
    pub resource: String,
    pub redirect_uri: String,
    pub scope: String,
    pub code_challenge: String,
    pub cookie_hash: String,
    pub private_payload: Value,
    pub user_id: Option<Uuid>,
    pub code_hash: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub struct OAuthGrant {
    pub id: Uuid,
    pub user_id: Uuid,
    pub client_id: String,
    pub client_name: String,
    pub resource: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub struct CredentialIssue<'a> {
    pub access_hash: &'a str,
    pub refresh_hash: Option<&'a str>,
    pub key_id: &'a str,
    pub version: i32,
    pub access_expires_at: DateTime<Utc>,
    pub grant_expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
}

pub enum ExchangeOutcome {
    Issued(OAuthGrant),
    Rejected(&'static str),
}

#[derive(Debug, FromRow)]
struct DeviceCode {
    client_id: String,
    resource: String,
    scope: String,
    expires_at: DateTime<Utc>,
    next_poll_at: DateTime<Utc>,
    poll_interval_seconds: i32,
    user_id: Option<Uuid>,
    denied_at: Option<DateTime<Utc>>,
    consumed_at: Option<DateTime<Utc>>,
}

impl OAuthRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert_login_flow(
        &self,
        id: Uuid,
        cookie_hash: &str,
        issuer: &str,
        payload: Value,
        expires: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO oidc_login_flows (id,cookie_hash,issuer,private_payload,expires_at) VALUES ($1,$2,$3,$4,$5)")
            .bind(id).bind(cookie_hash).bind(issuer).bind(payload).bind(expires)
            .execute(&self.pool).await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn consume_login_flow(
        &self,
        id: Uuid,
        cookie_hash: &str,
        issuer: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Value>> {
        sqlx::query_scalar("DELETE FROM oidc_login_flows WHERE id=$1 AND cookie_hash=$2 AND issuer=$3 AND expires_at>$4 RETURNING private_payload")
            .bind(id).bind(cookie_hash).bind(issuer).bind(now)
            .fetch_optional(&self.pool).await.map_err(map_sqlx_error)
    }

    pub async fn insert_authorization(&self, auth: &Authorization) -> Result<()> {
        sqlx::query("INSERT INTO oauth_authorizations (id,client_id,client_name,resource,redirect_uri,scope,code_challenge,cookie_hash,private_payload,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(auth.id).bind(&auth.client_id).bind(&auth.client_name).bind(&auth.resource).bind(&auth.redirect_uri)
            .bind(&auth.scope).bind(&auth.code_challenge).bind(&auth.cookie_hash).bind(&auth.private_payload).bind(auth.expires_at)
            .execute(&self.pool).await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn find_authorization(
        &self,
        id: Uuid,
        cookie_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Authorization>> {
        sqlx::query_as("SELECT * FROM oauth_authorizations WHERE id=$1 AND cookie_hash=$2 AND expires_at>$3 AND consumed_at IS NULL AND code_hash IS NULL")
            .bind(id).bind(cookie_hash).bind(now).fetch_optional(&self.pool).await.map_err(map_sqlx_error)
    }

    pub async fn approve_authorization(
        &self,
        id: Uuid,
        cookie_hash: &str,
        user: Uuid,
        code_hash: &str,
        now: DateTime<Utc>,
        expires: DateTime<Utc>,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !lock_user(&mut tx, user).await? {
            return Ok(false);
        }
        let changed=sqlx::query("UPDATE oauth_authorizations SET user_id=$3,code_hash=$4,expires_at=$6 WHERE id=$1 AND cookie_hash=$2 AND expires_at>$5 AND code_hash IS NULL AND consumed_at IS NULL")
            .bind(id).bind(cookie_hash).bind(user).bind(code_hash).bind(now).bind(expires)
            .execute(&mut *tx).await.map_err(map_sqlx_error)?.rows_affected()==1;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(changed)
    }

    pub async fn deny_authorization(&self, id: Uuid, cookie_hash: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM oauth_authorizations WHERE id=$1 AND cookie_hash=$2 AND code_hash IS NULL",
        )
        .bind(id)
        .bind(cookie_hash)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn exchange_code(
        &self,
        code_hash: &str,
        client: &str,
        resource: &str,
        redirect: &str,
        challenge: &str,
        issue: &CredentialIssue<'_>,
        now: DateTime<Utc>,
    ) -> Result<ExchangeOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let owner: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM oauth_authorizations WHERE code_hash=$1 AND user_id IS NOT NULL",
        )
        .bind(code_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let Some(user) = owner else {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        };
        if !lock_user(&mut tx, user).await? {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        let auth: Authorization =
            sqlx::query_as("SELECT * FROM oauth_authorizations WHERE code_hash=$1 FOR UPDATE")
                .bind(code_hash)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if auth.client_id != client || auth.resource != resource || auth.redirect_uri != redirect || auth.code_challenge != challenge {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        if auth.consumed_at.is_some() {
            let grant: Option<OAuthGrant> = sqlx::query_as("SELECT * FROM oauth_grants WHERE id=$1 FOR UPDATE")
                .bind(auth.id).fetch_optional(&mut *tx).await.map_err(map_sqlx_error)?;
            if let Some(grant)=grant { revoke_grant(&mut tx,&grant,now,"code_reuse").await?; }
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        if auth.expires_at <= now { return Ok(ExchangeOutcome::Rejected("invalid_grant")); }
        sqlx::query("UPDATE oauth_authorizations SET consumed_at=$2 WHERE id=$1")
            .bind(auth.id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        let grant = new_grant(
            &mut tx,
            auth.id,
            user,
            &auth.client_id,
            &auth.client_name,
            &auth.resource,
            &auth.scope,
            issue,
            now,
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(ExchangeOutcome::Issued(grant))
    }

    pub async fn rotate_refresh(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        issue: &CredentialIssue<'_>,
        now: DateTime<Utc>,
    ) -> Result<ExchangeOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let owner:Option<(Uuid,Uuid)>=sqlx::query_as("SELECT g.user_id,g.id FROM oauth_credentials c JOIN oauth_grants g ON g.id=c.grant_id WHERE c.token_hash=$1 AND c.kind='refresh'")
            .bind(hash).fetch_optional(&mut *tx).await.map_err(map_sqlx_error)?;
        let Some((user, id)) = owner else {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        };
        if !lock_user(&mut tx, user).await? {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        let grant: OAuthGrant = sqlx::query_as("SELECT * FROM oauth_grants WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        if grant.client_id != client || grant.resource != resource || !grant.is_live(now) {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        let (expires, consumed): (DateTime<Utc>, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT expires_at,consumed_at FROM oauth_credentials WHERE token_hash=$1",
        )
        .bind(hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if consumed.is_some() {
            revoke_grant(&mut tx, &grant, now, "refresh_reuse").await?;
            // This is deliberately committed: returning an error must not undo revocation.
            tx.commit().await.map_err(map_sqlx_error)?;
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        if expires <= now {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        sqlx::query("UPDATE oauth_credentials SET consumed_at=$2 WHERE token_hash=$1")
            .bind(hash)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
        let grant: OAuthGrant = sqlx::query_as(
            "UPDATE oauth_grants SET idle_expires_at=LEAST(expires_at,$2) WHERE id=$1 RETURNING *",
        )
        .bind(id)
        .bind(issue.idle_expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        insert_credentials(&mut tx, &grant, issue).await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(ExchangeOutcome::Issued(grant))
    }

    pub async fn access_user(
        &self,
        hash: &str,
        resource: &str,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Uuid>> {
        sqlx::query_scalar("SELECT g.user_id FROM oauth_credentials c JOIN oauth_grants g ON g.id=c.grant_id JOIN accounts a ON a.id=g.user_id WHERE c.token_hash=$1 AND c.kind='access' AND c.expires_at>$4 AND g.resource=$2 AND $3=ANY(string_to_array(g.scope,' ')) AND g.revoked_at IS NULL AND g.expires_at>$4 AND g.idle_expires_at>$4 AND a.is_active=true AND a.deleted_at IS NULL")
            .bind(hash).bind(resource).bind(scope).bind(now).fetch_optional(&self.pool).await.map_err(map_sqlx_error)
    }

    pub async fn revoke_token(&self, hash: &str, client: &str, now: DateTime<Utc>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let grant:Option<OAuthGrant>=sqlx::query_as("SELECT g.* FROM oauth_grants g JOIN oauth_credentials c ON c.grant_id=g.id WHERE c.token_hash=$1 AND g.client_id=$2 FOR UPDATE OF g")
            .bind(hash).bind(client).fetch_optional(&mut *tx).await.map_err(map_sqlx_error)?;
        if let Some(grant) = grant {
            revoke_grant(&mut tx, &grant, now, "client_revoked").await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn list_grants(&self, user: Uuid, now: DateTime<Utc>) -> Result<Vec<OAuthGrant>> {
        sqlx::query_as("SELECT * FROM oauth_grants WHERE user_id=$1 AND revoked_at IS NULL AND expires_at>$2 AND idle_expires_at>$2 ORDER BY created_at DESC LIMIT 100")
            .bind(user).bind(now).fetch_all(&self.pool).await.map_err(map_sqlx_error)
    }

    pub async fn revoke_owned_grant(&self, id: Uuid, user: Uuid, now: DateTime<Utc>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let grant: Option<OAuthGrant> =
            sqlx::query_as("SELECT * FROM oauth_grants WHERE id=$1 AND user_id=$2 FOR UPDATE")
                .bind(id)
                .bind(user)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        if let Some(grant) = grant {
            revoke_grant(&mut tx, &grant, now, "user_revoked").await?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn insert_device(
        &self,
        device_hash: &str,
        user_code_hash: &str,
        client: &str,
        resource: &str,
        scope: &str,
        now: DateTime<Utc>,
        expires: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO oauth_device_codes (device_hash,user_code_hash,client_id,resource,scope,expires_at,next_poll_at) VALUES ($1,$2,$3,$4,$5,$7,$6)")
            .bind(device_hash).bind(user_code_hash).bind(client).bind(resource).bind(scope).bind(now).bind(expires)
            .execute(&self.pool).await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn approve_device(
        &self,
        user_code_hash: &str,
        user: Uuid,
        approve: bool,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        if !lock_user(&mut tx, user).await? {
            return Ok(false);
        }
        let changed=sqlx::query("UPDATE oauth_device_codes SET user_id=CASE WHEN $3 THEN $2 ELSE NULL END,denied_at=CASE WHEN $3 THEN NULL ELSE $4 END WHERE user_code_hash=$1 AND expires_at>$4 AND consumed_at IS NULL AND user_id IS NULL AND denied_at IS NULL")
            .bind(user_code_hash).bind(user).bind(approve).bind(now).execute(&mut *tx).await.map_err(map_sqlx_error)?.rows_affected()==1;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(changed)
    }

    pub async fn poll_device(
        &self,
        hash: &str,
        client: &str,
        resource: &str,
        issue: &CredentialIssue<'_>,
        now: DateTime<Utc>,
    ) -> Result<ExchangeOutcome> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        // Account lock precedes credential locks, matching account deletion and refresh.
        let owner: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM oauth_device_codes WHERE device_hash=$1 AND user_id IS NOT NULL",
        )
        .bind(hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if let Some(user) = owner
            && !lock_user(&mut tx, user).await?
        {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        let device: Option<DeviceCode> =
            sqlx::query_as("SELECT * FROM oauth_device_codes WHERE device_hash=$1 FOR UPDATE")
                .bind(hash)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
        let Some(device) = device else {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        };
        if device.client_id != client || device.resource != resource || device.consumed_at.is_some()
        {
            return Ok(ExchangeOutcome::Rejected("invalid_grant"));
        }
        if device.expires_at <= now {
            return Ok(ExchangeOutcome::Rejected("expired_token"));
        }
        if device.denied_at.is_some() {
            return Ok(ExchangeOutcome::Rejected("access_denied"));
        }
        let too_fast = now < device.next_poll_at;
        let interval = if too_fast {
            device.poll_interval_seconds.saturating_add(5).min(300)
        } else {
            device.poll_interval_seconds
        };
        sqlx::query("UPDATE oauth_device_codes SET next_poll_at=$2,poll_interval_seconds=$3 WHERE device_hash=$1")
            .bind(hash).bind(now+chrono::Duration::seconds(i64::from(interval))).bind(interval)
            .execute(&mut *tx).await.map_err(map_sqlx_error)?;
        let outcome = if too_fast {
            ExchangeOutcome::Rejected("slow_down")
        } else if let Some(user) = device.user_id {
            // If approval raced the initial owner lookup, retry on the next poll; do not invert locks.
            if owner != Some(user) {
                ExchangeOutcome::Rejected("authorization_pending")
            } else {
                sqlx::query("UPDATE oauth_device_codes SET consumed_at=$2 WHERE device_hash=$1")
                    .bind(hash)
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_error)?;
                ExchangeOutcome::Issued(
                    new_grant(
                        &mut tx,
                        Uuid::new_v4(),
                        user,
                        client,
                        "NoteGate CLI",
                        resource,
                        &device.scope,
                        issue,
                        now,
                    )
                    .await?,
                )
            }
        } else {
            ExchangeOutcome::Rejected("authorization_pending")
        };
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(outcome)
    }
}

impl OAuthGrant {
    fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > now && self.idle_expires_at > now
    }
}

async fn lock_user(conn: &mut PgConnection, user: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_,Uuid>("SELECT id FROM accounts WHERE id=$1 AND kind='user' AND is_active=true AND deleted_at IS NULL FOR SHARE")
        .bind(user).fetch_optional(conn).await.map_err(map_sqlx_error)?.is_some())
}

async fn new_grant(
    conn: &mut PgConnection,
    id: Uuid,
    user: Uuid,
    client: &str,
    name: &str,
    resource: &str,
    scope: &str,
    issue: &CredentialIssue<'_>,
    now: DateTime<Utc>,
) -> Result<OAuthGrant> {
    let offline=scope.split_whitespace().any(|s|s=="offline_access");
    let expires=if offline {issue.grant_expires_at} else {issue.access_expires_at};
    let idle=issue.idle_expires_at.min(expires);
    let grant:OAuthGrant=sqlx::query_as("INSERT INTO oauth_grants (id,user_id,client_id,client_name,resource,scope,created_at,expires_at,idle_expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *")
        .bind(id).bind(user).bind(client).bind(name).bind(resource).bind(scope).bind(now).bind(expires).bind(idle)
        .fetch_one(&mut *conn).await.map_err(map_sqlx_error)?;
    insert_credentials(conn, &grant, issue).await?;
    crate::audit_events::oauth_grant_changed(conn, user, grant.id, "oauth.approve", None).await?;
    Ok(grant)
}

async fn insert_credentials(
    conn: &mut PgConnection,
    grant: &OAuthGrant,
    issue: &CredentialIssue<'_>,
) -> Result<()> {
    for (kind, hash, expires) in [
        (
            "access",
            Some(issue.access_hash),
            issue.access_expires_at.min(grant.expires_at),
        ),
        (
            "refresh",
            issue.refresh_hash.filter(|_| {
                grant
                    .scope
                    .split_whitespace()
                    .any(|s| s == "offline_access")
            }),
            grant.expires_at,
        ),
    ] {
        if let Some(hash) = hash {
            sqlx::query("INSERT INTO oauth_credentials (token_hash,hash_key_id,hash_version,grant_id,kind,expires_at) VALUES ($1,$2,$3,$4,$5,$6)")
                .bind(hash).bind(issue.key_id).bind(issue.version).bind(grant.id).bind(kind).bind(expires)
                .execute(&mut *conn).await.map_err(map_sqlx_error)?;
        }
    }
    Ok(())
}

async fn revoke_grant(
    conn: &mut PgConnection,
    grant: &OAuthGrant,
    now: DateTime<Utc>,
    reason: &str,
) -> Result<()> {
    if grant.revoked_at.is_none() {
        sqlx::query("UPDATE oauth_grants SET revoked_at=$2 WHERE id=$1")
            .bind(grant.id)
            .bind(now)
            .execute(&mut *conn)
            .await
            .map_err(map_sqlx_error)?;
        crate::audit_events::oauth_grant_changed(
            conn,
            grant.user_id,
            grant.id,
            "oauth.revoke",
            Some(reason),
        )
        .await?;
    }
    Ok(())
}
