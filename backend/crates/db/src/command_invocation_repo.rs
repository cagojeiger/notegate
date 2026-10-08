//! Best-effort external command invocation history persistence.

use chrono::{DateTime, Utc};
use notegate_core::{
    Error, Result,
    security::{EncryptedHistoryValue, PiiCrypto},
};
use notegate_model::{CommandInvocation, CommandInvocationCursor, CommandInvocationSurface};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct CommandInvocationRepo {
    pool: PgPool,
    crypto: PiiCrypto,
}

#[derive(Debug)]
pub struct NewCommandInvocation<'a> {
    pub invocation_id: Option<Uuid>,
    pub owner_user_id: Uuid,
    pub actor_account_id: Uuid,
    pub caller_kind: &'static str,
    pub surface: &'static str,
    pub tool: &'a str,
    pub op: Option<&'a str>,
    pub purpose: Option<&'a str>,
    pub space_name: Option<&'a str>,
    pub input: &'a Value,
    pub response: Option<&'a Value>,
    pub outcome: &'static str,
    pub error_code: Option<&'a str>,
    pub duration_ms: i64,
}

impl CommandInvocationRepo {
    #[cfg(any(test, feature = "test-util"))]
    pub fn new(pool: PgPool) -> Self {
        Self::with_crypto(pool, PiiCrypto::test())
    }

    pub fn with_crypto(pool: PgPool, crypto: PiiCrypto) -> Self {
        Self { pool, crypto }
    }

    pub async fn insert(&self, invocation: NewCommandInvocation<'_>) -> Result<()> {
        validate_private_fields(&invocation)?;
        let snapshot_id = Uuid::new_v4();
        let private = PrivatePayload {
            purpose: invocation.purpose.map(str::to_owned),
            space_name: invocation.space_name.map(str::to_owned),
            input: invocation.input.clone(),
            response: invocation.response.cloned(),
        };
        let encrypted = self.protect(invocation.owner_user_id, snapshot_id, &private)?;
        sqlx::query(
            "INSERT INTO command_invocations \
             (owner_user_id, actor_account_id, caller_kind, surface, tool, op, input, outcome, error_code, duration_ms, snapshot_id, private_payload, invocation_id) \
             VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb, $7, $8, $9, $10, $11, $12)",
        )
        .bind(invocation.owner_user_id)
        .bind(invocation.actor_account_id)
        .bind(invocation.caller_kind)
        .bind(invocation.surface)
        .bind(invocation.tool)
        .bind(invocation.op)
        .bind(invocation.outcome)
        .bind(invocation.error_code)
        .bind(invocation.duration_ms)
        .bind(snapshot_id)
        .bind(encrypted)
        .bind(invocation.invocation_id)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    fn protect(&self, owner: Uuid, id: Uuid, payload: &PrivatePayload) -> Result<Value> {
        let plaintext = serde_json::to_string(payload)
            .map_err(|_| Error::internal("invocation history encoding failed"))?;
        serde_json::to_value(
            self.crypto
                .encrypt_history(&binding(owner, id), &plaintext)?,
        )
        .map_err(|_| Error::internal("invocation history encoding failed"))
    }

    /// Bounded, restartable migration, including legacy writes during a rolling deploy.
    pub async fn encrypt_legacy_payloads(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;
        let rows = sqlx::query_as::<_, CommandInvocationRow>(
            "SELECT id, created_at, owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, \
                    space_name, input, response, outcome, error_code, duration_ms, snapshot_id, private_payload, invocation_id \
             FROM command_invocations WHERE private_payload IS NULL ORDER BY id LIMIT 100 FOR UPDATE SKIP LOCKED",
        ).fetch_all(&mut *tx).await.map_err(map_sqlx_error)?;
        let count = rows.len() as u64;
        for row in rows {
            let snapshot_id = Uuid::new_v4();
            let private = PrivatePayload {
                purpose: row.purpose,
                space_name: row.space_name,
                input: row.input,
                response: row.response,
            };
            let encrypted = self.protect(row.owner_user_id, snapshot_id, &private)?;
            sqlx::query("UPDATE command_invocations SET purpose=NULL, space_name=NULL, input='{}'::jsonb, response=NULL, snapshot_id=$2, private_payload=$3 WHERE id=$1")
                .bind(row.id).bind(snapshot_id).bind(encrypted)
                .execute(&mut *tx).await.map_err(map_sqlx_error)?;
        }
        tx.commit().await.map_err(map_sqlx_error)?;
        Ok(count)
    }

    pub async fn list_by_owner(
        &self,
        owner_user_id: Uuid,
        surface: CommandInvocationSurface,
        limit: i64,
        cursor: Option<&CommandInvocationCursor>,
    ) -> Result<Vec<CommandInvocation>> {
        let cursor_created_at = cursor.map(|cursor| cursor.created_at);
        let cursor_id = cursor.map(|cursor| cursor.id);
        let rows = sqlx::query_as::<_, CommandInvocationRow>(
            "SELECT id, created_at, owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, \
                    space_name, input, response, outcome, error_code, duration_ms, snapshot_id, private_payload, invocation_id \
             FROM command_invocations \
             WHERE owner_user_id = $1 \
               AND surface = $2 \
               AND ($3::timestamptz IS NULL OR (created_at, id) < ($3, $4)) \
             ORDER BY created_at DESC, id DESC \
             LIMIT $5",
        )
        .bind(owner_user_id)
        .bind(surface.as_str())
        .bind(cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        rows.into_iter()
            .map(|mut row| {
                if let Some(private) = row.private_payload.take() {
                    let snapshot_id = row.snapshot_id.ok_or_else(|| {
                        Error::internal("encrypted invocation has no snapshot identity")
                    })?;
                    let encrypted: EncryptedHistoryValue = serde_json::from_value(private)
                        .map_err(|_| Error::internal("invalid encrypted invocation history"))?;
                    let plaintext = self
                        .crypto
                        .decrypt_history(&binding(row.owner_user_id, snapshot_id), &encrypted)?;
                    let payload: PrivatePayload = serde_json::from_str(&plaintext)
                        .map_err(|_| Error::internal("invalid invocation history payload"))?;
                    row.purpose = payload.purpose;
                    row.space_name = payload.space_name;
                    row.input = payload.input;
                    row.response = payload.response;
                }
                Ok(CommandInvocation::from(row))
            })
            .collect()
    }
}

fn binding(owner: Uuid, id: Uuid) -> String {
    format!("invocations/{owner}/{id}")
}

#[derive(Serialize, Deserialize)]
struct PrivatePayload {
    purpose: Option<String>,
    space_name: Option<String>,
    input: Value,
    response: Option<Value>,
}

// Encrypted values cannot be checked by PostgreSQL; preserve the existing
// contract before encryption. Structural constraints remain enforced in SQL.
fn validate_private_fields(invocation: &NewCommandInvocation<'_>) -> Result<()> {
    if !invocation.input.is_object() || invocation.response.is_some_and(|value| !value.is_object())
    {
        return Err(Error::validation("invocation payloads must be objects"));
    }
    if invocation.purpose.is_some_and(|value| {
        value.is_empty() || value.chars().count() > 200 || value.trim() != value
    }) {
        return Err(Error::validation("invalid invocation purpose"));
    }
    if let Some(name) = invocation.space_name {
        if invocation.tool != "read" || invocation.op != Some("changes") {
            return Err(Error::validation(
                "space name is only recorded for read changes",
            ));
        }
        notegate_core::validation::validate_space_name(name)
            .map_err(|_| Error::validation("invalid invocation space name"))?;
    }
    Ok(())
}

#[derive(Debug, FromRow)]
struct CommandInvocationRow {
    invocation_id: Option<Uuid>,
    id: i64,
    owner_user_id: Uuid,
    created_at: DateTime<Utc>,
    actor_account_id: Uuid,
    caller_kind: String,
    surface: String,
    tool: String,
    op: Option<String>,
    purpose: Option<String>,
    space_name: Option<String>,
    input: Value,
    response: Option<Value>,
    outcome: String,
    error_code: Option<String>,
    duration_ms: i64,
    snapshot_id: Option<Uuid>,
    private_payload: Option<Value>,
}

impl From<CommandInvocationRow> for CommandInvocation {
    fn from(row: CommandInvocationRow) -> Self {
        Self {
            id: row.id,
            invocation_id: row.invocation_id,
            created_at: row.created_at,
            actor_account_id: row.actor_account_id,
            caller_kind: row.caller_kind,
            surface: row.surface,
            tool: row.tool,
            op: row.op,
            purpose: row.purpose,
            space_name: row.space_name,
            input: row.input,
            response: row.response,
            outcome: row.outcome,
            error_code: row.error_code,
            duration_ms: row.duration_ms,
        }
    }
}
