//! File change event history queries for file-tree changes.

use notegate_core::limits;
use notegate_db::FileChangeSyncRows;
use notegate_model::{
    FileChangeEventCursor, FileChangeEventIdCursor, FileChangeEventPage, FileChangeSyncPage,
    ListFileChangeEvents, ListFileChangeEventsById, SyncFileChanges,
};
use uuid::Uuid;

use crate::pagination::{clamp_limit, paginate_keyset};
use crate::{ServiceError, ServiceResult};

use super::{FileCommand, FilesService};

fn shape_file_change_sync_page(
    batch: FileChangeSyncRows,
    after_id: Option<i64>,
    limit: i64,
) -> FileChangeSyncPage {
    if !batch.token_valid {
        return FileChangeSyncPage {
            items: Vec::new(),
            limit,
            next_after_id: batch.latest_id,
            has_more: false,
            resync_required: true,
        };
    }

    let mut items = batch.events;
    let has_more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next_after_id = items
        .last()
        .map(|event| event.id)
        .or(after_id)
        .unwrap_or(batch.latest_id);

    FileChangeSyncPage {
        items,
        limit,
        next_after_id,
        has_more,
        resync_required: false,
    }
}

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

    /// List mutation events by `id DESC` for MCP history. This intentionally
    /// does not alter the existing REST history cursor or display-time order.
    pub async fn list_file_change_events_by_id(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: ListFileChangeEventsById,
    ) -> ServiceResult<FileChangeEventPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;

        let (items, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<FileChangeEventIdCursor>| async move {
                if cursor.as_ref().is_some_and(|cursor| {
                    cursor.space_id != space_id || cursor.node_id != request.node_id
                }) {
                    return Err(ServiceError::InvalidInput(
                        "change history cursor does not match this scope".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .list_file_change_events_by_id(
                        space_id,
                        request.node_id,
                        limit,
                        cursor.map(|cursor| cursor.id),
                    )
                    .await?)
            },
            |event| FileChangeEventIdCursor {
                space_id,
                node_id: request.node_id,
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

        Ok(shape_file_change_sync_page(batch, request.after_id, limit))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use notegate_model::FileChangeEvent;
    use serde_json::Value;

    use super::*;

    fn event(id: i64) -> FileChangeEvent {
        FileChangeEvent {
            id,
            created_at: Utc::now(),
            space_id: Uuid::nil(),
            node_id: None,
            actor_account_id: None,
            op_type: "test".to_owned(),
            metadata: Value::Null,
        }
    }

    fn batch(ids: &[i64], latest_id: i64, token_valid: bool) -> FileChangeSyncRows {
        FileChangeSyncRows {
            events: ids.iter().copied().map(event).collect(),
            latest_id,
            token_valid,
        }
    }

    #[test]
    fn invalid_sync_token_requires_resync_from_latest_id() {
        let page = shape_file_change_sync_page(batch(&[1000], 42, false), Some(999), 10);

        assert!(page.items.is_empty());
        assert_eq!(page.next_after_id, 42);
        assert!(!page.has_more);
        assert!(page.resync_required);
    }

    #[test]
    fn empty_sync_page_uses_the_available_anchor() {
        let initial = shape_file_change_sync_page(batch(&[], 42, true), None, 10);
        assert_eq!(initial.next_after_id, 42);
        assert!(!initial.has_more);
        assert!(!initial.resync_required);

        let continuation = shape_file_change_sync_page(batch(&[], 100, true), Some(42), 10);
        assert_eq!(continuation.next_after_id, 42);
        assert!(!continuation.has_more);
        assert!(!continuation.resync_required);
    }

    #[test]
    fn exact_sync_page_ends_at_its_last_event() {
        let page = shape_file_change_sync_page(batch(&[11, 12], 12, true), Some(10), 2);

        assert_eq!(
            page.items.iter().map(|event| event.id).collect::<Vec<_>>(),
            vec![11, 12]
        );
        assert_eq!(page.next_after_id, 12);
        assert!(!page.has_more);
        assert!(!page.resync_required);
    }

    #[test]
    fn sync_page_truncates_lookahead_and_reports_more() {
        let page = shape_file_change_sync_page(batch(&[11, 12, 13], 13, true), Some(10), 2);

        assert_eq!(
            page.items.iter().map(|event| event.id).collect::<Vec<_>>(),
            vec![11, 12]
        );
        assert_eq!(page.next_after_id, 12);
        assert!(page.has_more);
        assert!(!page.resync_required);
    }
}
