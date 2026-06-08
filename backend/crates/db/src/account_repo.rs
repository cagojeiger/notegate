//! Accounts + users persistence (replaces the old single-user `user_repo`).
//!
//! All queries use runtime-checked `query_as::<_, Row>()` / `query()` — never
//! the `query!` macro — so a schema reset never breaks compilation. The user
//! account self-registers: there is no `accounts.created_by`, so the upsert
//! inserts `accounts(kind='user')` then `users(id, sub, email)` in one
//! transaction, or updates the existing row when the `sub` already exists.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use crate::map_sqlx_error;
use notegate_core::{Error, Result};
use notegate_model::account::{Account, AccountKind, AccountRef};
use notegate_model::user::User;
use notegate_service::identity::{AccountStore, ResolveAttrs, UserStore};
use sqlx::{FromRow, PgPool, Row as _};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AccountRepo {
    pool: PgPool,
}

impl AccountRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A row from `accounts`.
#[derive(Debug, FromRow)]
struct AccountRow {
    id: Uuid,
    kind: String,
    display_name: String,
    is_active: bool,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AccountRow {
    fn into_account(self) -> Result<Account> {
        let kind = AccountKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown account kind: {}", self.kind)))?;
        Ok(Account {
            id: self.id,
            kind,
            display_name: self.display_name,
            is_active: self.is_active,
            deleted_at: self.deleted_at,
            deleted_by: self.deleted_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// A row from `users`.
#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    sub: Option<String>,
    email: Option<String>,
    anonymized_at: Option<DateTime<Utc>>,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            sub: row.sub,
            email: row.email,
            anonymized_at: row.anonymized_at,
        }
    }
}

const ACCOUNT_COLUMNS: &str =
    "id, kind, display_name, is_active, deleted_at, deleted_by, created_at, updated_at";
const USER_COLUMNS: &str = "id, sub, email, anonymized_at";

impl AccountRepo {
    /// Load the account+user pair for an account id, if it is a user account.
    pub async fn find_caller_by_account_id(
        &self,
        account_id: Uuid,
    ) -> Result<Option<(Account, User)>> {
        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(account_row) = account_row else {
            return Ok(None);
        };
        let account = account_row.into_account()?;

        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        match user_row {
            Some(user_row) => Ok(Some((account, User::from(user_row)))),
            None => Ok(None),
        }
    }

    /// Resolve a set of account ids to lightweight [`AccountRef`]s in one query,
    /// keyed by id. Ids with no matching account are simply absent from the map.
    /// Used by the REST layer to fill the `created_by`/`updated_by` and access
    /// account fields without an N+1 lookup per row.
    pub async fn find_account_refs(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, AccountRef>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ANY($1)"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let account = row.into_account()?;
            map.insert(account.id, AccountRef::from(&account));
        }
        Ok(map)
    }

    /// Soft-delete and deactivate an account, anonymizing its user PII.
    ///
    /// Used by account teardown; recorded with the acting account in
    /// `deleted_by`. Kept here so user lifecycle stays in one repo.
    pub async fn anonymize_user(&self, account_id: Uuid, deleted_by: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE accounts \
             SET is_active = false, deleted_at = now(), deleted_by = $2, updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(account_id)
        .bind(deleted_by)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        sqlx::query(
            "UPDATE users SET sub = NULL, email = NULL, anonymized_at = now() WHERE id = $1",
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(())
    }
}

impl AccountStore for AccountRepo {
    async fn find_account(&self, id: Uuid) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(AccountRow::into_account).transpose()
    }
}

impl UserStore for AccountRepo {
    async fn upsert_user_by_sub(&self, attrs: &ResolveAttrs) -> Result<(Account, User)> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        // Serialize self-registration for the same external subject. Without
        // this, two first-logins for the same `sub` can both observe no user,
        // both insert an account, and one then hits the unique users.sub
        // constraint after creating an orphan account row.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&attrs.sub)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;

        // Look up an existing user account by sub inside the transaction.
        let existing_id: Option<Uuid> = sqlx::query("SELECT id FROM users WHERE sub = $1")
            .bind(&attrs.sub)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx_error)?
            .map(|row| row.get::<Uuid, _>("id"));

        let account_id = match existing_id {
            // Existing sub: reactivate and refresh the display name; never a
            // duplicate account.
            Some(id) => {
                sqlx::query(
                    "UPDATE accounts \
                     SET display_name = $2, is_active = true, deleted_at = NULL, \
                         deleted_by = NULL, updated_at = now() \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(&attrs.name)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query("UPDATE users SET email = $2 WHERE id = $1")
                    .bind(id)
                    .bind(&attrs.email)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_error)?;
                id
            }
            // New sub: self-register the account, then its user detail.
            None => {
                let id: Uuid = sqlx::query(
                    "INSERT INTO accounts (kind, display_name) \
                     VALUES ('user', $1) RETURNING id",
                )
                .bind(&attrs.name)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?
                .get::<Uuid, _>("id");

                sqlx::query("INSERT INTO users (id, sub, email) VALUES ($1, $2, $3)")
                    .bind(id)
                    .bind(&attrs.sub)
                    .bind(&attrs.email)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_error)?;
                id
            }
        };

        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id = $1"
        ))
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        tx.commit().await.map_err(map_sqlx_error)?;
        Ok((account_row.into_account()?, User::from(user_row)))
    }

    async fn find_user_by_sub(&self, sub: &str) -> Result<Option<(Account, User)>> {
        let user_row = sqlx::query_as::<_, UserRow>(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE sub = $1"
        ))
        .bind(sub)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        let Some(user_row) = user_row else {
            return Ok(None);
        };

        let account_row = sqlx::query_as::<_, AccountRow>(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = $1"
        ))
        .bind(user_row.id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(Some((account_row.into_account()?, User::from(user_row))))
    }
}
