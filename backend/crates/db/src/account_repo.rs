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

/// FROM/WHERE selecting the live workspaces an account is the SOLE active-user owner
/// of (no other active user owner exists). `$1` binds the account id. Shared by the
/// service deletion gate and the repository transaction invariant so both operate on
/// the same set.
const SOLE_OWNED_WORKSPACES_FROM_WHERE: &str = "\
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
      )";

#[derive(Debug, Clone)]
pub struct AccountRepo {
    pool: PgPool,
    crypto: PiiCrypto,
}

impl AccountRepo {
    #[cfg(any(test, feature = "test-util"))]
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
                let Some(key_id) = self.display_name_enc_key_id.as_ref() else {
                    return Ok(String::new());
                };
                // Stored enc version must match the active crypto version before
                // decryption, so a mismatch is a clear error, not an opaque AEAD failure.
                let stored = self.display_name_enc_version;
                if stored != Some(crypto.version()) {
                    return Err(Error::internal(format!(
                        "PII enc version mismatch: stored {stored:?} != current {}",
                        crypto.version()
                    )));
                }
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
        let email = match self.email_enc_key_id.as_ref() {
            Some(key_id) => {
                // Stored enc version must match the active crypto version before
                // decryption, so a mismatch is a clear error, not an opaque AEAD failure.
                let stored = self.email_enc_version;
                if stored != Some(crypto.version()) {
                    return Err(Error::internal(format!(
                        "PII enc version mismatch: stored {stored:?} != current {}",
                        crypto.version()
                    )));
                }
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

    /// Soft-delete a user account (ADR 0004). Mark it deleted and tear down owned
    /// agents, keys, and access, but do not delete workspaces. If the user is the
    /// sole active owner of any live workspace, reject so the caller must delete or
    /// transfer those workspaces first. KEEP `provider_sub_hash` and PII as a
    /// tombstone. The purge run anonymizes PII and frees the sub-hash once the
    /// retention window elapses; re-login during the window is rejected by
    /// `upsert_user_by_sub`, so a returning sub never duplicates the account.
    pub async fn soft_delete_user(&self, account_id: Uuid, deleted_by: Uuid) -> Result<()> {
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

        let _locked_owner_workspaces: Vec<Uuid> = sqlx::query_scalar(
            "SELECT w.id \
             FROM workspace_access wa \
             JOIN workspaces w ON w.id = wa.workspace_id AND w.deleted_at IS NULL \
             WHERE wa.account_id = $1 AND wa.role = 'owner' AND wa.revoked_at IS NULL \
             FOR UPDATE OF w",
        )
        .bind(account_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let sole_owned: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) {SOLE_OWNED_WORKSPACES_FROM_WHERE}"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        if sole_owned > 0 {
            return Err(Error::conflict(format!(
                "delete or transfer your {sole_owned} owned workspace(s) before deleting your account"
            )));
        }

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
               AND (account_id = $1 OR account_id = ANY($2))",
        )
        .bind(account_id)
        .bind(&owned_agents)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        // Soft-delete only: keep display_name and the users-row PII/`provider_sub_hash`
        // as a tombstone. The purge run anonymizes PII and frees the sub-hash later.
        sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = $1 AND kind = 'user'",
        )
        .bind(account_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }

    /// Count live workspaces this account is the SOLE active user owner of. ADR 0004:
    /// the account cannot be deleted until these are deleted or their ownership is
    /// transferred. Co-owned workspaces (another active user owner exists) do not count.
    pub async fn count_sole_owned_workspaces(&self, account_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) {SOLE_OWNED_WORKSPACES_FROM_WHERE}"
        ))
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(count)
    }

    pub async fn find_account(&self, id: Uuid) -> Result<Option<Account>> {
        self.fetch_account_row(id)
            .await?
            .map(|row| row.into_account(&self.crypto))
            .transpose()
    }

    pub async fn upsert_user_by_sub(&self, attrs: &ResolveAttrs) -> Result<(Account, User)> {
        validate_resolve_attrs(attrs)?;
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
            // ADR 0004: a soft-deleted (pending-deletion) account still holds its
            // `provider_sub_hash` tombstone. Reject re-registration until the purge run
            // erases it — never resurrect or duplicate. After purge the tombstone is
            // gone, so the sub no longer matches and falls through to a fresh account.
            Some((_id, false)) => {
                return Err(Error::conflict(
                    "account is pending deletion; re-registration is available once it is fully erased",
                ));
            }
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

fn validate_resolve_attrs(attrs: &ResolveAttrs) -> Result<()> {
    if attrs.sub.trim().is_empty() {
        return Err(Error::validation("provider subject cannot be empty"));
    }
    if attrs.sub.chars().count() > limits::OAUTH_PROVIDER_SUB_MAX_CHARS {
        return Err(Error::validation(format!(
            "provider subject exceeds the maximum of {} characters",
            limits::OAUTH_PROVIDER_SUB_MAX_CHARS
        )));
    }
    if attrs.name.trim().is_empty() {
        return Err(Error::validation("user display name cannot be empty"));
    }
    if attrs.name.chars().count() > limits::USER_DISPLAY_NAME_MAX_CHARS {
        return Err(Error::validation(format!(
            "user display name exceeds the maximum of {} characters",
            limits::USER_DISPLAY_NAME_MAX_CHARS
        )));
    }
    if attrs.email.trim().is_empty() {
        return Err(Error::validation("user email cannot be empty"));
    }
    if attrs.email.chars().count() > limits::USER_EMAIL_MAX_CHARS {
        return Err(Error::validation(format!(
            "user email exceeds the maximum of {} characters",
            limits::USER_EMAIL_MAX_CHARS
        )));
    }
    Ok(())
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
