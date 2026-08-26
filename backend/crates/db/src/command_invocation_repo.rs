//! Best-effort external command invocation history persistence.

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::{CommandInvocation, CommandInvocationCursor, CommandInvocationSurface};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct CommandInvocationRepo {
    pool: PgPool,
}

#[derive(Debug)]
pub struct NewCommandInvocation<'a> {
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
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, invocation: NewCommandInvocation<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO command_invocations \
             (owner_user_id, actor_account_id, caller_kind, surface, tool, op, purpose, space_name, input, response, outcome, error_code, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(invocation.owner_user_id)
        .bind(invocation.actor_account_id)
        .bind(invocation.caller_kind)
        .bind(invocation.surface)
        .bind(invocation.tool)
        .bind(invocation.op)
        .bind(invocation.purpose)
        .bind(invocation.space_name)
        .bind(invocation.input)
        .bind(invocation.response)
        .bind(invocation.outcome)
        .bind(invocation.error_code)
        .bind(invocation.duration_ms)
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
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
            "SELECT id, created_at, actor_account_id, caller_kind, surface, tool, op, purpose, \
                    space_name, input, response, outcome, error_code, duration_ms \
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
        Ok(rows.into_iter().map(CommandInvocation::from).collect())
    }
}

#[derive(Debug, FromRow)]
struct CommandInvocationRow {
    id: i64,
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
}

impl From<CommandInvocationRow> for CommandInvocation {
    fn from(row: CommandInvocationRow) -> Self {
        Self {
            id: row.id,
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
