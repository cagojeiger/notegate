//! Accounts + users persistence.
//!
//! User PII is never stored as plaintext. Display names and email addresses are
//! encrypted with a per-account DEK; provider subjects and email lookup keys are
//! HMAC hashes. Agent display names are product metadata and are derived from
//! `agents.name`.

use std::collections::HashMap;

use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::security::{EncryptedField, PiiCrypto};
use notegate_core::{Error, Result};
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
    is_active: bool,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    agent_name: Option<String>,
    wrapped_dek: Option<Vec<u8>>,
}

impl AccountRow {
    fn user_dek(&self, crypto: &PiiCrypto) -> Result<Option<[u8; 32]>> {
        self.wrapped_dek
            .as_deref()
            .map(|wrapped| crypto.unwrap_dek(wrapped))
            .transpose()
    }

    fn display_name(&self, crypto: &PiiCrypto) -> Result<String> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        match kind {
            AccountKind::Agent => Ok(self.agent_name.clone().unwrap_or_default()),
            AccountKind::User => {
                let Some(dek) = self.user_dek(crypto)? else {
                    return Ok(String::new());
                };
                decrypt_optional_string(
                    crypto,
                    &dek,
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
    anonymized_at: Option<DateTime<Utc>>,
}

impl UserRow {
    fn into_user(self, crypto: &PiiCrypto, dek: Option<&[u8; 32]>) -> Result<User> {
        let email = match dek {
            Some(dek) => decrypt_optional_string(
                crypto,
                dek,
                self.email_ciphertext.as_ref(),
                self.email_nonce.as_ref(),
            )?,
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
     a.is_active, a.deleted_at, a.deleted_by, a.created_at, a.updated_at, \
     ag.name AS agent_name, k.wrapped_dek";
const USER_COLUMNS: &str = "id, email_ciphertext, email_nonce, anonymized_at";

impl AccountRepo {
    pub async fn find_caller_by_account_id(
        &self,
        account_id: Uuid,
    ) -> Result<Option<(Account, User)>> {
        let account_row = self.fetch_account_row(account_id).await?;
        let Some(account_row) = account_row else {
            return Ok(None);
        };
        let dek = account_row.user_dek(&self.crypto)?;
        let account = account_row.into_account(&self.crypto)?;

        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        match user_row {
            Some(user_row) => Ok(Some((
                account,
                user_row.into_user(&self.crypto, dek.as_ref())?,
            ))),
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
             LEFT JOIN account_encryption_keys k ON k.account_id = a.id \
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

        sqlx::query(
            "UPDATE accounts \
             SET display_name_ciphertext = NULL, display_name_nonce = NULL, \
                 is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = $1 AND kind = 'user' AND deleted_at IS NULL",
        )
        .bind(account_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE users \
             SET provider_sub_hash = NULL, email_ciphertext = NULL, email_nonce = NULL, \
                 email_hash = NULL, anonymized_at = now() \
             WHERE id = $1",
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE account_encryption_keys \
             SET wrapped_dek = NULL, updated_at = now(), destroyed_at = now() \
             WHERE account_id = $1 AND destroyed_at IS NULL",
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

        let existing_id: Option<Uuid> =
            sqlx::query("SELECT id FROM users WHERE provider_sub_hash = $1")
                .bind(&provider_sub_hash)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?
                .map(|row| row.get::<Uuid, _>("id"));

        let account_id = match existing_id {
            Some(id) => {
                let dek = self.fetch_dek_for_update(&mut tx, id).await?;
                let display_name = self.crypto.encrypt_string(&dek, &attrs.name)?;
                let email = self.crypto.encrypt_string(&dek, &attrs.email)?;

                sqlx::query(
                    "UPDATE accounts \
                     SET display_name_ciphertext = $2, display_name_nonce = $3, \
                         is_active = true, deleted_at = NULL, deleted_by = NULL, updated_at = now() \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(display_name.ciphertext)
                .bind(display_name.nonce)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query(
                    "UPDATE users \
                     SET email_ciphertext = $2, email_nonce = $3, email_hash = $4, \
                         email_hash_version = $5, anonymized_at = NULL \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(email.ciphertext)
                .bind(email.nonce)
                .bind(&email_hash)
                .bind(self.crypto.hash_version())
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
                id
            }
            None => {
                let dek = self.crypto.generate_dek();
                let wrapped_dek = self.crypto.wrap_dek(&dek)?;
                let display_name = self.crypto.encrypt_string(&dek, &attrs.name)?;
                let email = self.crypto.encrypt_string(&dek, &attrs.email)?;

                let id: Uuid = sqlx::query(
                    "INSERT INTO accounts (kind, display_name_ciphertext, display_name_nonce) \
                     VALUES ('user', $1, $2) RETURNING id",
                )
                .bind(display_name.ciphertext)
                .bind(display_name.nonce)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?
                .get::<Uuid, _>("id");

                sqlx::query(
                    "INSERT INTO account_encryption_keys \
                     (account_id, kek_id, kek_version, wrapped_dek) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(id)
                .bind(self.crypto.kek_id())
                .bind(self.crypto.kek_version())
                .bind(wrapped_dek)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query(
                    "INSERT INTO users \
                     (id, provider_sub_hash, provider_sub_hash_version, email_ciphertext, email_nonce, email_hash, email_hash_version) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7)",
                )
                .bind(id)
                .bind(&provider_sub_hash)
                .bind(self.crypto.hash_version())
                .bind(email.ciphertext)
                .bind(email.nonce)
                .bind(&email_hash)
                .bind(self.crypto.hash_version())
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
             LEFT JOIN account_encryption_keys k ON k.account_id = a.id \
             WHERE a.id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let dek = account_row.user_dek(&self.crypto)?;
        let account = account_row.into_account(&self.crypto)?;
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((account, user_row.into_user(&self.crypto, dek.as_ref())?))
    }

    pub async fn find_user_by_sub(&self, sub: &str) -> Result<Option<(Account, User)>> {
        let provider_sub_hash = self.crypto.provider_sub_hash(AUTH_PROVIDER, sub)?;
        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             JOIN users u ON u.id = a.id \
             LEFT JOIN agents ag ON ag.id = a.id \
             LEFT JOIN account_encryption_keys k ON k.account_id = a.id \
             WHERE u.provider_sub_hash = $1"
        ))
        .bind(&provider_sub_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(account_row) = account_row else {
            return Ok(None);
        };
        let dek = account_row.user_dek(&self.crypto)?;
        let account = account_row.into_account(&self.crypto)?;
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account.id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(Some((
            account,
            user_row.into_user(&self.crypto, dek.as_ref())?,
        )))
    }

    async fn fetch_account_row(&self, account_id: Uuid) -> Result<Option<AccountRow>> {
        sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} \
             FROM accounts a \
             LEFT JOIN agents ag ON ag.id = a.id \
             LEFT JOIN account_encryption_keys k ON k.account_id = a.id \
             WHERE a.id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    async fn fetch_dek_for_update(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_id: Uuid,
    ) -> Result<[u8; 32]> {
        let wrapped: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT wrapped_dek FROM account_encryption_keys \
             WHERE account_id = $1 AND destroyed_at IS NULL \
             FOR UPDATE",
        )
        .bind(account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx_error)?;
        let wrapped = wrapped.ok_or_else(|| Error::internal("account DEK not found"))?;
        self.crypto.unwrap_dek(&wrapped)
    }
}

fn decrypt_optional_string(
    crypto: &PiiCrypto,
    dek: &[u8; 32],
    ciphertext: Option<&Vec<u8>>,
    nonce: Option<&Vec<u8>>,
) -> Result<Option<String>> {
    match (ciphertext, nonce) {
        (Some(ciphertext), Some(nonce)) => crypto
            .decrypt_string(
                dek,
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
