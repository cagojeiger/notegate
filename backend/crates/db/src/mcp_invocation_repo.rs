//! Best-effort MCP invocation history persistence.

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::{McpInvocation, McpInvocationCursor};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::event_history_query::{EventCursorPosition, UuidFilter, list_event_rows};
use crate::map_sqlx_error;

#[derive(Debug, Clone)]
pub struct McpInvocationRepo {
    pool: PgPool,
}

#[derive(Debug)]
pub struct NewMcpInvocation<'a> {
    pub owner_user_id: Uuid,
    pub actor_account_id: Uuid,
    pub caller_kind: &'static str,
    pub tool: &'static str,
    pub op: Option<&'a str>,
    pub purpose: Option<&'a str>,
    pub outcome: &'static str,
    pub error_code: Option<&'a str>,
    pub duration_ms: i64,
}

impl McpInvocationRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, invocation: NewMcpInvocation<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO mcp_invocations \
             (owner_user_id, actor_account_id, caller_kind, tool, op, purpose, outcome, error_code, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(invocation.owner_user_id)
        .bind(invocation.actor_account_id)
        .bind(invocation.caller_kind)
        .bind(invocation.tool)
        .bind(invocation.op)
        .bind(invocation.purpose)
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
        limit: i64,
        cursor: Option<&McpInvocationCursor>,
    ) -> Result<Vec<McpInvocation>> {
        let rows = list_event_rows::<McpInvocationRow>(
            &self.pool,
            "mcp_invocations",
            MCP_INVOCATION_COLUMNS,
            UuidFilter::new("owner_user_id", owner_user_id),
            None,
            limit,
            cursor.map(|cursor| EventCursorPosition {
                created_at: cursor.created_at,
                id: cursor.id,
            }),
        )
        .await?;
        Ok(rows.into_iter().map(McpInvocation::from).collect())
    }
}

#[derive(Debug, FromRow)]
struct McpInvocationRow {
    id: i64,
    created_at: DateTime<Utc>,
    actor_account_id: Uuid,
    caller_kind: String,
    tool: String,
    op: Option<String>,
    purpose: Option<String>,
    outcome: String,
    error_code: Option<String>,
    duration_ms: i64,
}

impl From<McpInvocationRow> for McpInvocation {
    fn from(row: McpInvocationRow) -> Self {
        Self {
            id: row.id,
            created_at: row.created_at,
            actor_account_id: row.actor_account_id,
            caller_kind: row.caller_kind,
            tool: row.tool,
            op: row.op,
            purpose: row.purpose,
            outcome: row.outcome,
            error_code: row.error_code,
            duration_ms: row.duration_ms,
        }
    }
}

const MCP_INVOCATION_COLUMNS: &str = "id, created_at, actor_account_id, caller_kind, tool, op, purpose, outcome, error_code, duration_ms";
