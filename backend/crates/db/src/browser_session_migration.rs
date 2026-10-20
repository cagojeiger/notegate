//! Offline preparation for migration 62; never called from requests or normal startup.
use crate::{CryptoKeyEpochRepo, map_sqlx_error};
use notegate_core::{Error, Result, security::PiiCrypto};
use sqlx::PgPool;

#[derive(Debug)]
pub struct SessionMigrationPreparation {
    pub recorded: u64,
    pub revoked: u64,
}

/// Run with API/workers stopped and the previously deployed, trusted OIDC issuer.
/// Preserve provenance in new Audit events and permanently reject untrusted sessions.
pub async fn prepare_browser_sessions(
    pool: &PgPool,
    crypto: &PiiCrypto,
    trusted_issuer: &str,
) -> Result<SessionMigrationPreparation> {
    if trusted_issuer.is_empty() || trusted_issuer.len() > 2048 {
        return Err(Error::validation("invalid previous OIDC issuer"));
    }
    CryptoKeyEpochRepo::new(pool.clone())
        .verify_active(crypto)
        .await?;
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    sqlx::query("LOCK TABLE browser_sessions IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
    let changed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM browser_sessions s JOIN audit_events a \
         ON a.resource_id=s.id AND a.op_type='session.migrate' \
         AND a.metadata->>'reason'='provider_independent' \
         WHERE a.metadata->>'trusted_issuer' IS DISTINCT FROM $1 \
            OR a.metadata->'issuer' IS DISTINCT FROM coalesce(to_jsonb(s.oidc_issuer),'null'::jsonb))",
    )
    .bind(trusted_issuer)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if changed {
        return Err(Error::validation(
            "session migration provenance has changed",
        ));
    }
    let recorded = sqlx::query(
        "INSERT INTO audit_events \
         (owner_user_id,actor_account_id,source,op_type,resource_type,resource_id,metadata) \
         SELECT s.user_id,NULL,'system','session.migrate','browser_session',s.id, \
                jsonb_build_object('issuer',s.oidc_issuer,'trusted_issuer',$1::text,'reason','provider_independent') \
         FROM browser_sessions s WHERE NOT EXISTS \
           (SELECT 1 FROM audit_events a WHERE a.resource_id=s.id AND a.op_type='session.migrate' \
            AND a.metadata->>'reason'='provider_independent')",
    )
    .bind(trusted_issuer)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    let revoked = sqlx::query(
        "WITH rejected AS (UPDATE browser_sessions SET revoked_at=now(), \
             revoked_reason='untrusted_login_issuer',updated_at=now() \
             WHERE revoked_at IS NULL AND oidc_issuer IS DISTINCT FROM $1 \
             RETURNING id,user_id) \
         INSERT INTO audit_events \
         (owner_user_id,actor_account_id,source,op_type,resource_type,resource_id,metadata) \
         SELECT user_id,NULL,'system','session.revoke','browser_session',id, \
                jsonb_build_object('reason','untrusted_login_issuer') FROM rejected",
    )
    .bind(trusted_issuer)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();
    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(SessionMigrationPreparation { recorded, revoked })
}
