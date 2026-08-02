//! MCP invocation history pagination for user self-review.

use notegate_core::limits;
use notegate_db::McpInvocationRepo;
use notegate_model::{ListMcpInvocations, McpInvocationCursor, McpInvocationPage};
use uuid::Uuid;

use crate::ServiceResult;
use crate::pagination::paginate_keyset;

pub async fn list_mcp_invocation_page(
    invocations: &McpInvocationRepo,
    owner_user_id: Uuid,
    request: ListMcpInvocations,
) -> ServiceResult<McpInvocationPage> {
    let (items, limit, has_more, next_cursor) = paginate_keyset(
        request.limit,
        limits::MCP_INVOCATIONS_DEFAULT_LIMIT,
        limits::MCP_INVOCATIONS_MAX_LIMIT,
        request.cursor.as_deref(),
        |limit, cursor: Option<McpInvocationCursor>| async move {
            Ok(invocations
                .list_by_owner(owner_user_id, limit, cursor.as_ref())
                .await?)
        },
        |invocation| McpInvocationCursor {
            created_at: invocation.created_at,
            id: invocation.id,
        },
    )
    .await?;

    Ok(McpInvocationPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}
