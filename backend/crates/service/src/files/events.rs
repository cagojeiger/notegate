//! File change event history queries for file-tree changes.

use notegate_core::limits;
use notegate_model::{FileChangeEventCursor, FileChangeEventPage, ListFileChangeEvents};
use uuid::Uuid;

use crate::pagination::clamp_limit;
use crate::{ServiceError, ServiceResult, cursor};

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

        let limit = clamp_limit(
            request.limit,
            limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
        );
        let cursor = match request.cursor.as_deref() {
            None => None,
            Some(raw) => Some(
                cursor::decode::<FileChangeEventCursor>(raw)
                    .map_err(|_error| ServiceError::InvalidInput("invalid cursor".to_owned()))?,
            ),
        };

        let mut items = self
            .store
            .list_file_change_events(space_id, request.node_id, limit + 1, cursor.as_ref())
            .await?;
        let has_more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items
                .last()
                .map(|event| FileChangeEventCursor {
                    created_at: event.created_at,
                    id: event.id,
                })
                .map(|cursor| cursor::encode(&cursor))
                .transpose()
                .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
        } else {
            None
        };

        Ok(FileChangeEventPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }
}
