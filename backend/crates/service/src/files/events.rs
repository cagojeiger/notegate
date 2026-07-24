//! File change event history queries for file-tree changes.

use notegate_core::limits;
use notegate_model::{
    FileChangeEventCursor, FileChangeEventPage, FileChangeSyncPage, ListFileChangeEvents,
    SyncFileChanges,
};
use uuid::Uuid;

use crate::ServiceResult;
use crate::pagination::{clamp_limit, paginate_keyset};

use super::{FileCommand, FilesService};

impl FilesService {
    /// List space-scoped file change event history. Requires read/stat access to the space.
    pub async fn list_file_change_events(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: ListFileChangeEvents,
    ) -> ServiceResult<FileChangeEventPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;

        let (items, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<FileChangeEventCursor>| async move {
                Ok(self
                    .store
                    .list_file_change_events(space_id, request.node_id, limit, cursor.as_ref())
                    .await?)
            },
            |event| FileChangeEventCursor {
                created_at: event.created_at,
                id: event.id,
            },
        )
        .await?;

        Ok(FileChangeEventPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// Establish or continue a lossless forward sync token for one Space.
    pub async fn sync_file_changes(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: SyncFileChanges,
    ) -> ServiceResult<FileChangeSyncPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;

        let limit = clamp_limit(
            request.limit,
            limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
        );
        let batch = self
            .store
            .sync_file_change_events(space_id, request.after_id, limit + 1)
            .await?;

        if !batch.token_valid {
            return Ok(FileChangeSyncPage {
                items: Vec::new(),
                next_after_id: batch.latest_id,
                has_more: false,
                resync_required: true,
            });
        }

        let mut items = batch.events;
        let has_more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next_after_id = items
            .last()
            .map(|event| event.id)
            .or(request.after_id)
            .unwrap_or(batch.latest_id);

        Ok(FileChangeSyncPage {
            items,
            next_after_id,
            has_more,
            resync_required: false,
        })
    }
}
