//! Best-effort MCP invocation history persistence.

use notegate_core::Result;
use sqlx::PgPool;
use uuid::Uuid;

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
}
