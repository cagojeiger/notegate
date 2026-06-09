//! Accounts + users persistence.
//!
//! User PII is never stored as plaintext. Display names and email addresses are
//! encrypted directly with the active ENC root-derived PII field key; provider
//! subjects and email lookup keys are HMAC hashes. Agent display names are
//! product metadata and are derived from `agents.name`.

use std::collections::HashMap;

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::security::{EncryptedField, PiiAad, PiiCrypto, PiiFieldKind};
use notegate_core::{Error, Result, limits};
use notegate_model::ResolveAttrs;
use notegate_model::account::{Account, AccountKind, AccountRef};
use notegate_model::user::User;
use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

const AUTH_PROVIDER: &str = "authgate";

#[derive(Debug, Clone)]
pub struct AccountRepo {
    pool: PgPool,
    crypto: PiiCrypto,
}

impl AccountRepo {
    pub fn new(pool: PgPool) -> Self {
        Self::with_crypto(pool, PiiCrypto::test())
    }

    pub fn with_crypto(pool: PgPool, crypto: PiiCrypto) -> Self {
        Self { pool, crypto }
    }
}

#[derive(Debug, FromRow)]
struct AccountRow {
    id: Uuid,
    kind: String,
    display_name_ciphertext: Option<Vec<u8>>,
    display_name_nonce: Option<Vec<u8>>,
    display_name_enc_key_id: Option<String>,
    display_name_enc_version: Option<i32>,
    is_active: bool,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    agent_name: Option<String>,
}

impl AccountRow {
    fn display_name(&self, crypto: &PiiCrypto) -> Result<String> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        match kind {
            AccountKind::Agent => Ok(self.agent_name.clone().unwrap_or_default()),
            AccountKind::User => {
                let _version = self.display_name_enc_version;
                let Some(key_id) = self.display_name_enc_key_id.as_ref() else {
                    return Ok(String::new());
                };
                let aad = PiiAad::new(
                    PiiFieldKind::AccountDisplayName,
                    self.id.to_string(),
                    key_id,
                );
                decrypt_optional_string(
                    crypto,
                    &aad,
                    self.display_name_ciphertext.as_ref(),
                    self.display_name_nonce.as_ref(),
                )
                .map(|value| value.unwrap_or_default())
            }
        }
    }

    fn into_account(self, crypto: &PiiCrypto) -> Result<Account> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        let display_name = self.display_name(crypto)?;
        Ok(Account {
            id: self.id,
            kind,
            display_name,
            is_active: self.is_active,
            deleted_at: self.deleted_at,
            deleted_by: self.deleted_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    email_ciphertext: Option<Vec<u8>>,
    email_nonce: Option<Vec<u8>>,
    email_enc_key_id: Option<String>,
    email_enc_version: Option<i32>,
    anonymized_at: Option<DateTime<Utc>>,
}

impl UserRow {
    fn into_user(self, crypto: &PiiCrypto) -> Result<User> {
        let _version = self.email_enc_version;
        let email = match self.email_enc_key_id.as_ref() {
            Some(key_id) => {
                let aad = PiiAad::new(PiiFieldKind::UserEmail, self.id.to_string(), key_id);
                decrypt_optional_string(
                    crypto,
                    &aad,
                    self.email_ciphertext.as_ref(),
                    self.email_nonce.as_ref(),
                )?
            }
            None => None,
        };
        Ok(User {
            id: self.id,
            email,
            anonymized_at: self.anonymized_at,
        })
    }
}

const ACCOUNT_COLUMNS: &str = "a.id, a.kind, a.display_name_ciphertext, a.display_name_nonce, \
     a.display_name_enc_key_id, a.display_name_enc_version, \
     a.is_active, a.deleted_at, a.deleted_by, a.created_at, a.updated_at, \
     ag.name AS agent_name";
const USER_COLUMNS: &str = "id, email_ciphertext, email_nonce, email_enc_key_id, \
     email_enc_version, anonymized_at";

impl AccountRepo {
    pub async fn find_caller_by_account_id(
        &self,
        account_id: Uuid,
    ) -> Result<Option<(Account, User)>> {
        let account_row = self.fetch_account_row(account_id).await?;
        let Some(account_row) = account_row else {
            return Ok(None);
        };
        let account = account_row.into_account(&self.crypto)?;

        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        match user_row {
            Some(user_row) => Ok(Some((account, user_row.into_user(&self.crypto)?))),
            None => Ok(None),
        }
    }

    pub async fn find_account_refs(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, AccountRef>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             LEFT JOIN agents ag ON ag.id = a.id \
             WHERE a.id = ANY($1)"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let account = row.into_account(&self.crypto)?;
            map.insert(account.id, AccountRef::from(&account));
        }
        Ok(map)
    }

    pub async fn anonymize_user(&self, account_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let locked: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM accounts \
             WHERE id = $1 AND kind = 'user' AND is_active = true AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if locked.is_none() {
            return Err(Error::not_found("active user account not found"));
        }

        let owned_workspaces: Vec<Uuid> = sqlx::query_scalar(
            "SELECT w.id \
             FROM workspace_access wa \
             JOIN workspaces w ON w.id = wa.workspace_id AND w.deleted_at IS NULL \
             WHERE wa.account_id = $1 AND wa.role = 'owner' AND wa.revoked_at IS NULL \
               AND NOT EXISTS ( \
                   SELECT 1 FROM workspace_access other \
                   JOIN accounts other_acc ON other_acc.id = other.account_id \
                   WHERE other.workspace_id = w.id \
                     AND other.account_id <> $1 \
                     AND other.role = 'owner' \
                     AND other.revoked_at IS NULL \
                     AND other_acc.kind = 'user' \
                     AND other_acc.is_active = true \
                     AND other_acc.deleted_at IS NULL \
               ) \
             FOR UPDATE OF w",
        )
        .bind(account_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let owned_agents: Vec<Uuid> = sqlx::query_scalar(
            "SELECT a.id FROM agents a \
             JOIN accounts acc ON acc.id = a.id \
             WHERE a.created_by = $1 AND acc.is_active = true AND acc.deleted_at IS NULL \
             FOR UPDATE OF acc",
        )
        .bind(account_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE workspaces \
             SET deleted_at = now(), deleted_by = $2, \
                 purge_after = now() + make_interval(days => $3::int), updated_at = now() \
             WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&owned_workspaces)
        .bind(deleted_by)
        .bind(i32::try_from(limits::DELETED_NODE_RETENTION_DAYS).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = ANY($1) AND kind = 'agent' AND deleted_at IS NULL",
        )
        .bind(&owned_agents)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE api_keys \
             SET revoked_at = now(), revoked_by = $2 \
             WHERE revoked_at IS NULL \
               AND (account_id = $1 OR account_id = ANY($3))",
        )
        .bind(account_id)
        .bind(deleted_by)
        .bind(&owned_agents)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE workspace_access \
             SET revoked_at = now(), revoked_by = $3 \
             WHERE revoked_at IS NULL \
               AND (account_id = $1 OR account_id = ANY($2) OR workspace_id = ANY($4))",
        )
        .bind(account_id)
        .bind(&owned_agents)
        .bind(deleted_by)
        .bind(&owned_workspaces)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE accounts \
             SET display_name_ciphertext = NULL, display_name_nonce = NULL, \
                 display_name_enc_key_id = NULL, display_name_enc_version = NULL, \
                 is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = $1 AND kind = 'user'",
        )
        .bind(account_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE users \
             SET provider_sub_hash = NULL, provider_sub_hash_key_id = NULL, provider_sub_hash_version = NULL, \
                email_ciphertext = NULL, email_nonce = NULL, email_enc_key_id = NULL, email_enc_version = NULL, \
                email_hash = NULL, email_hash_key_id = NULL, email_hash_version = NULL, anonymized_at = now() \
             WHERE id = $1",
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    pub async fn find_account(&self, id: Uuid) -> Result<Option<Account>> {
        self.fetch_account_row(id)
            .await?
            .map(|row| row.into_account(&self.crypto))
            .transpose()
    }

    pub async fn upsert_user_by_sub(&self, attrs: &ResolveAttrs) -> Result<(Account, User)> {
        let provider_sub_hash = self.crypto.provider_sub_hash(AUTH_PROVIDER, &attrs.sub)?;
        let email_hash = self.crypto.email_hash(&attrs.email)?;
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&provider_sub_hash)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        let existing: Option<(Uuid, bool)> = sqlx::query_as(
            "SELECT u.id, a.is_active \
                 FROM users u \
                 JOIN accounts a ON a.id = u.id \
                 WHERE u.provider_sub_hash = $1",
        )
        .bind(&provider_sub_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let account_id = match existing {
            Some((id, false)) => id,
            Some((id, true)) => {
                let display_aad = PiiAad::new(
                    PiiFieldKind::AccountDisplayName,
                    id.to_string(),
                    self.crypto.enc_key_id(),
                );
                let email_aad = PiiAad::new(
                    PiiFieldKind::UserEmail,
                    id.to_string(),
                    self.crypto.enc_key_id(),
                );
                let display_name = self.crypto.encrypt_pii_string(&display_aad, &attrs.name)?;
                let email = self.crypto.encrypt_pii_string(&email_aad, &attrs.email)?;

                sqlx::query(
                    "UPDATE accounts \
                     SET display_name_ciphertext = $2, display_name_nonce = $3, \
                         display_name_enc_key_id = $4, display_name_enc_version = $5, \
                         is_active = true, deleted_at = NULL, deleted_by = NULL, updated_at = now() \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(display_name.ciphertext)
                .bind(display_name.nonce)
                .bind(self.crypto.enc_key_id())
                .bind(self.crypto.version())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query(
                    "UPDATE users \
                     SET email_ciphertext = $2, email_nonce = $3, email_enc_key_id = $4, \
                         email_enc_version = $5, email_hash = $6, email_hash_key_id = $7, \
                         email_hash_version = $8, anonymized_at = NULL \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(email.ciphertext)
                .bind(email.nonce)
                .bind(self.crypto.enc_key_id())
                .bind(self.crypto.version())
                .bind(&email_hash)
                .bind(self.crypto.lookup_key_id())
                .bind(self.crypto.version())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
                id
            }
            None => {
                let id: Uuid =
                    sqlx::query("INSERT INTO accounts (kind) VALUES ('user') RETURNING id")
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(map_sqlx_error)?
                        .get::<Uuid, _>("id");

                let display_aad = PiiAad::new(
                    PiiFieldKind::AccountDisplayName,
                    id.to_string(),
                    self.crypto.enc_key_id(),
                );
                let email_aad = PiiAad::new(
                    PiiFieldKind::UserEmail,
                    id.to_string(),
                    self.crypto.enc_key_id(),
                );
                let display_name = self.crypto.encrypt_pii_string(&display_aad, &attrs.name)?;
                let email = self.crypto.encrypt_pii_string(&email_aad, &attrs.email)?;

                sqlx::query(
                    "UPDATE accounts \
                     SET display_name_ciphertext = $2, display_name_nonce = $3, \
                         display_name_enc_key_id = $4, display_name_enc_version = $5, updated_at = now() \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(display_name.ciphertext)
                .bind(display_name.nonce)
                .bind(self.crypto.enc_key_id())
                .bind(self.crypto.version())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query(
                    "INSERT INTO users \
                     (id, provider_sub_hash, provider_sub_hash_key_id, provider_sub_hash_version, \
                      email_ciphertext, email_nonce, email_enc_key_id, email_enc_version, \
                      email_hash, email_hash_key_id, email_hash_version) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                )
                .bind(id)
                .bind(&provider_sub_hash)
                .bind(self.crypto.lookup_key_id())
                .bind(self.crypto.version())
                .bind(email.ciphertext)
                .bind(email.nonce)
                .bind(self.crypto.enc_key_id())
                .bind(self.crypto.version())
                .bind(&email_hash)
                .bind(self.crypto.lookup_key_id())
                .bind(self.crypto.version())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                id
            }
        };

        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             LEFT JOIN agents ag ON ag.id = a.id \
             WHERE a.id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let account = account_row.into_account(&self.crypto)?;
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((account, user_row.into_user(&self.crypto)?))
    }

    pub async fn find_user_by_sub(&self, sub: &str) -> Result<Option<(Account, User)>> {
        let provider_sub_hash = self.crypto.provider_sub_hash(AUTH_PROVIDER, sub)?;
        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             JOIN users u ON u.id = a.id \
             LEFT JOIN agents ag ON ag.id = a.id \
             WHERE u.provider_sub_hash = $1"
        ))
        .bind(&provider_sub_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(account_row) = account_row else {
            return Ok(None);
        };
        let account = account_row.into_account(&self.crypto)?;
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account.id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(Some((account, user_row.into_user(&self.crypto)?)))
    }

    async fn fetch_account_row(&self, account_id: Uuid) -> Result<Option<AccountRow>> {
        sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             LEFT JOIN agents ag ON ag.id = a.id \
             WHERE a.id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }
}

fn decrypt_optional_string(
    crypto: &PiiCrypto,
    aad: &PiiAad,
    ciphertext: Option<&Vec<u8>>,
    nonce: Option<&Vec<u8>>,
) -> Result<Option<String>> {
    match (ciphertext, nonce) {
        (Some(ciphertext), Some(nonce)) => crypto
            .decrypt_pii_string(
                aad,
                &EncryptedField {
                    ciphertext: ciphertext.clone(),
                    nonce: nonce.clone(),
                },
            )
            .map(Some),
        (None, None) => Ok(None),
        _ => Err(Error::internal("invalid encrypted field state")),
    }
}
