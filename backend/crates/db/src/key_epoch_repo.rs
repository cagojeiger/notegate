//! Crypto key epoch startup ensure and verification.
//!
//! The database stores only epoch metadata and verify tags. Root secrets and
//! derived subkeys stay in the application process and are supplied by
//! `notegate_core::security::PiiCrypto`.

use crate::map_sqlx_error;
use notegate_core::security::PiiCrypto;
use notegate_core::{Error, Result};
use sqlx::{FromRow, PgPool};

#[derive(Debug, Clone)]
pub struct CryptoKeyEpochRepo {
    pool: PgPool,
}

impl CryptoKeyEpochRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Ensure active ENC/LOOKUP epochs exist for the configured roots, then verify them.
    ///
    /// This is intentionally not rotation: if a different active key already
    /// exists for a domain, startup fails instead of replacing it.
    pub async fn ensure_active(&self, crypto: &PiiCrypto) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        ensure_active_epoch(
            &mut tx,
            crypto.enc_key_id(),
            "enc",
            &crypto.enc_epoch_verify_tag(crypto.enc_key_id())?,
            crypto.version(),
        )
        .await?;
        ensure_active_epoch(
            &mut tx,
            crypto.lookup_key_id(),
            "lookup",
            &crypto.lookup_epoch_verify_tag(crypto.lookup_key_id())?,
            crypto.version(),
        )
        .await?;
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Fail startup unless active ENC/LOOKUP rows match the configured roots.
    pub async fn verify_active(&self, crypto: &PiiCrypto) -> Result<()> {
        verify_active_epoch(
            &self.pool,
            crypto.enc_key_id(),
            "enc",
            &crypto.enc_epoch_verify_tag(crypto.enc_key_id())?,
            crypto.version(),
        )
        .await?;
        verify_active_epoch(
            &self.pool,
            crypto.lookup_key_id(),
            "lookup",
            &crypto.lookup_epoch_verify_tag(crypto.lookup_key_id())?,
            crypto.version(),
        )
        .await?;
        Ok(())
    }
}

#[derive(Debug, FromRow)]
struct EpochRow {
    key_id: String,
    domain: String,
    status: String,
    verify_tag: String,
    version: i32,
}

async fn ensure_active_epoch(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key_id: &str,
    domain: &str,
    verify_tag: &str,
    version: i32,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO crypto_key_epochs \
         (key_id, domain, status, verify_tag, version, activated_at) \
         VALUES ($1, $2, 'active', $3, $4, now()) \
         ON CONFLICT DO NOTHING",
    )
    .bind(key_id)
    .bind(domain)
    .bind(verify_tag)
    .bind(version)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;

    let active = sqlx::query_as::<_, EpochRow>(
        "SELECT key_id, domain, status, verify_tag, version FROM crypto_key_epochs \
         WHERE domain = $1 AND status = 'active'",
    )
    .bind(domain)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::validation(format!("missing active {domain} crypto key epoch")))?;

    if active.key_id != key_id {
        return Err(Error::validation(format!(
            "active {domain} crypto key epoch is {}, not configured {key_id}",
            active.key_id
        )));
    }

    validate_epoch_row(key_id, domain, verify_tag, version, active)
}

async fn verify_active_epoch(
    pool: &PgPool,
    key_id: &str,
    domain: &str,
    verify_tag: &str,
    version: i32,
) -> Result<()> {
    let row = sqlx::query_as::<_, EpochRow>(
        "SELECT key_id, domain, status, verify_tag, version FROM crypto_key_epochs WHERE key_id = $1",
    )
    .bind(key_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::validation(format!("missing active {domain} crypto key epoch")))?;
    validate_epoch_row(key_id, domain, verify_tag, version, row)
}

fn validate_epoch_row(
    key_id: &str,
    domain: &str,
    verify_tag: &str,
    version: i32,
    row: EpochRow,
) -> Result<()> {
    if row.key_id != key_id {
        return Err(Error::validation(format!(
            "crypto key epoch row has wrong key_id: expected {key_id}"
        )));
    }
    if row.domain != domain {
        return Err(Error::validation(format!(
            "crypto key epoch {key_id} has wrong domain"
        )));
    }
    if row.status != "active" {
        return Err(Error::validation(format!(
            "crypto key epoch {key_id} is not active"
        )));
    }
    if row.version != version || row.verify_tag != verify_tag {
        return Err(Error::validation(format!(
            "crypto key epoch {key_id} does not match configured secret"
        )));
    }
    Ok(())
}
