//! Issuer-scoped identity lookup and verified ownership links.

use notegate_core::{Error, Result};
use sqlx::PgConnection;
use uuid::Uuid;

/// Bind $1 = verified issuer, $2 = scoped hash. The primary key permits one owner.
pub(crate) const USER_ID_BY_IDENTITY: &str =
    "SELECT user_id FROM external_identities WHERE issuer = $1 AND subject_hash = $2";

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
