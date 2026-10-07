//! Dashboard trash operations are restricted to the active owner user.
use notegate_model::trash::{TrashCursor, TrashEntryVersion, TrashPage};
use notegate_model::{AccountKind, Caller, Channel};
use uuid::Uuid;

use super::FilesService;
use crate::pagination::paginate_keyset;
use crate::{ServiceError, ServiceResult};

fn require_dashboard_user(caller: &Caller) -> ServiceResult<()> {
    if caller.account.kind != AccountKind::User || caller.channel != Channel::Browser {
        return Err(ServiceError::Forbidden(
            "trash is only available to owner users in the dashboard".to_owned(),
        ));
    }
    Ok(())
}

impl FilesService {
    pub async fn list_trash(
        &self,
        caller: &Caller,
        limit: Option<i64>,
        cursor: Option<&str>,
    ) -> ServiceResult<TrashPage> {
        require_dashboard_user(caller)?;
        let owner = caller.account_id();
        let (items, limit, has_more, next_cursor) = paginate_keyset(
            limit,
            50,
            100,
            cursor,
            |limit, cursor: Option<TrashCursor>| async move {
                if cursor.as_ref().is_some_and(|c| c.owner_user_id != owner) {
                    return Err(ServiceError::InvalidInput(
                        "cursor does not belong to this owner".to_owned(),
                    ));
                }
                self.store
                    .list_trash(owner, limit, cursor)
                    .await
                    .map_err(Into::into)
            },
            |item| TrashCursor {
                owner_user_id: owner,
                deleted_at: item.deleted_at,
                id: item.id,
            },
        )
        .await?;
        Ok(TrashPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    pub async fn restore_trash(
        &self,
        caller: &Caller,
        space: Uuid,
        node: Option<Uuid>,
        expected: TrashEntryVersion,
    ) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        match node {
            Some(node) => {
                self.store
                    .restore_trashed_node(caller.account_id(), space, node, expected)
                    .await?
            }
            None => {
                self.store
                    .restore_trashed_space(caller.account_id(), space, expected)
                    .await?
            }
        }
        Ok(())
    }

    pub async fn purge_trash(
        &self,
        caller: &Caller,
        space: Uuid,
        node: Option<Uuid>,
        expected: TrashEntryVersion,
    ) -> ServiceResult<()> {
        require_dashboard_user(caller)?;
        self.store
            .request_trash_purge(caller.account_id(), space, node, expected)
            .await?;
        Ok(())
    }
}
