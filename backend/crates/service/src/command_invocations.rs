//! External command invocation history pagination for user self-review.

use notegate_core::limits;
use notegate_db::CommandInvocationRepo;
use notegate_model::{
    CommandInvocationCursor, CommandInvocationPage, CommandInvocationSurface,
    ListCommandInvocations,
};
use uuid::Uuid;

use crate::ServiceError;
use crate::ServiceResult;
use crate::pagination::paginate_keyset;

pub async fn list_command_invocation_page(
    invocations: &CommandInvocationRepo,
    owner_user_id: Uuid,
    request: ListCommandInvocations,
) -> ServiceResult<CommandInvocationPage> {
    let surface = request.surface;
    let (items, limit, has_more, next_cursor) = paginate_keyset(
        request.limit,
        limits::COMMAND_INVOCATIONS_DEFAULT_LIMIT,
        limits::COMMAND_INVOCATIONS_MAX_LIMIT,
        request.cursor.as_deref(),
        |limit, cursor: Option<CommandInvocationCursor>| async move {
            validate_cursor_surface(surface, cursor.as_ref())?;
            Ok(invocations
                .list_by_owner(owner_user_id, surface, limit, cursor.as_ref())
                .await?)
        },
        |invocation| CommandInvocationCursor {
            created_at: invocation.created_at,
            id: invocation.id,
            surface,
        },
    )
    .await?;

    Ok(CommandInvocationPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}

fn validate_cursor_surface(
    surface: CommandInvocationSurface,
    cursor: Option<&CommandInvocationCursor>,
) -> ServiceResult<()> {
    if cursor.is_some_and(|cursor| cursor.surface != surface) {
        return Err(ServiceError::InvalidInput("invalid cursor".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn cursor_cannot_cross_invocation_surfaces() {
        let cursor = CommandInvocationCursor {
            created_at: Utc::now(),
            id: 7,
            surface: CommandInvocationSurface::Mcp,
        };

        assert!(validate_cursor_surface(CommandInvocationSurface::Mcp, Some(&cursor)).is_ok());
        assert_eq!(
            validate_cursor_surface(CommandInvocationSurface::Cli, Some(&cursor)),
            Err(ServiceError::InvalidInput("invalid cursor".to_owned()))
        );
    }
}
