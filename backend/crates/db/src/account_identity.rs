//! Issuer-scoped identity lookup and the bounded legacy subject compatibility path.

use notegate_core::{Error, Result};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub(crate) const LEGACY_PROVIDER: &str = "authgate";

/// Bind $1 = verified issuer, $2 = scoped hash, $3 = legacy hash.
/// UNION also lets callers detect inconsistent mappings instead of choosing an owner.
pub(crate) const USER_IDS: &str = "\
    SELECT user_id FROM external_identities WHERE issuer = $1 AND subject_hash = $2 \
    UNION \
    SELECT id FROM users WHERE provider_sub_hash = $3 \
      AND EXISTS (SELECT 1 FROM legacy_identity_issuer WHERE issuer = $1)";

pub(crate) async fn ensure_legacy_issuer(pool: &PgPool, issuer: &str) -> Result<()> {
    if issuer.is_empty() || issuer.len() > 2048 {
        return Err(Error::validation("invalid legacy identity issuer"));
    }
    sqlx::query(
        "INSERT INTO legacy_identity_issuer (issuer) VALUES ($1) \
         ON CONFLICT (singleton) DO NOTHING",
    )
    .bind(issuer)
    .execute(pool)
    .await
    .map_err(crate::map_sqlx_error)?;
    let registered: String = sqlx::query_scalar("SELECT issuer FROM legacy_identity_issuer")
        .fetch_one(pool)
        .await
        .map_err(crate::map_sqlx_error)?;
    if registered != issuer {
        return Err(Error::validation(
            "legacy identity issuer cannot be changed; migrate identities before changing the OAuth issuer",
        ));
    }
    Ok(())
}

pub(crate) async fn is_legacy_issuer(conn: &mut PgConnection, issuer: &str) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM legacy_identity_issuer WHERE issuer = $1)")
        .bind(issuer)
        .fetch_one(conn)
        .await
        .map_err(crate::map_sqlx_error)
}

pub(crate) async fn insert_identity(
    conn: &mut PgConnection,
    user_id: Uuid,
    issuer: &str,
    subject_hash: &str,
    key_id: &str,
    version: i32,
) -> Result<()> {
    let inserted = sqlx::query(
        "INSERT INTO external_identities \
         (user_id, issuer, subject_hash, subject_hash_key_id, subject_hash_version) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (issuer, subject_hash) DO NOTHING",
    )
    .bind(user_id)
    .bind(issuer)
    .bind(subject_hash)
    .bind(key_id)
    .bind(version)
    .execute(&mut *conn)
    .await
    .map_err(crate::map_sqlx_error)?;
    if inserted.rows_affected() == 0 {
        let owner: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM external_identities WHERE issuer = $1 AND subject_hash = $2",
        )
        .bind(issuer)
        .bind(subject_hash)
        .fetch_one(conn)
        .await
        .map_err(crate::map_sqlx_error)?;
        if owner != user_id {
            return Err(Error::conflict("external identity is already linked"));
        }
    }
    Ok(())
}

pub(crate) fn only_row<T>(mut rows: Vec<T>) -> Result<Option<T>> {
    if rows.len() > 1 {
        return Err(Error::internal("external identity ownership is inconsistent"));
    }
    Ok(rows.pop())
}
