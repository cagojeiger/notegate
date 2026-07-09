//! File change event history queries for file-tree changes.

use notegate_core::limits;
use notegate_model::{FileChangeEventCursor, FileChangeEventPage, ListFileChangeEvents};
use uuid::Uuid;

use crate::ServiceResult;
use crate::pagination::paginate_keyset;

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
}
