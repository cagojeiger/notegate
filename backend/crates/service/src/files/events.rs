//! File change event history queries for file-tree changes.

use notegate_core::limits;
use notegate_db::FileChangeSyncRows;
use notegate_model::{
    FileChangeEventCursor, FileChangeEventIdCursor, FileChangeEventPage, FileChangeSyncPage,
    ListFileChangeEvents, ListFileChangeEventsById, SyncFileChanges,
};
use std::collections::HashSet;
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
    async fn filter_external_events(
        &self,
        space_id: Uuid,
        items: &mut Vec<notegate_model::FileChangeEvent>,
    ) -> ServiceResult<bool> {
        if self.channel == notegate_model::Channel::Browser || items.is_empty() {
            return Ok(false);
        }
        let ids: Vec<Uuid> = items
            .iter()
            .flat_map(event_node_ids)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let allowed = self
            .store
            .externally_accessible_node_ids(space_id, &ids, true)
            .await?;
        let original_len = items.len();
        items.retain(|event| {
            event.node_id.is_some() && event_node_ids(event).all(|id| allowed.contains(&id))
        });
        for event in items.iter_mut() {
            redact_subtree_counts(event);
        }
        Ok(items.len() != original_len)
    }

    /// List space-scoped file change event history. Requires read/stat access to the space.
    pub async fn list_file_change_events(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: ListFileChangeEvents,
    ) -> ServiceResult<FileChangeEventPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;

        let (mut items, limit, has_more, next_cursor) = paginate_keyset(
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

        self.filter_external_events(space_id, &mut items).await?;
        Ok(FileChangeEventPage {
            items,
            limit,
            has_more,
            next_cursor,
        })
    }

    /// List mutation events before an MCP changes cursor by `id DESC` without
    /// changing the REST display-time order.
    pub async fn list_file_change_events_by_id(
        &self,
        caller_account_id: Uuid,
        space_id: Uuid,
        request: ListFileChangeEventsById,
    ) -> ServiceResult<FileChangeEventPage> {
        self.authorize(space_id, caller_account_id, FileCommand::Stat)
            .await?;

        let (mut items, limit, has_more, next_cursor) = paginate_keyset(
            request.limit,
            limits::FILE_CHANGE_EVENTS_DEFAULT_LIMIT,
            limits::FILE_CHANGE_EVENTS_MAX_LIMIT,
            request.cursor.as_deref(),
            |limit, cursor: Option<FileChangeEventIdCursor>| async move {
                if cursor
                    .as_ref()
                    .is_some_and(|cursor| cursor.space_id != space_id)
                {
                    return Err(ServiceError::InvalidInput(
                        "change history cursor does not match this scope".to_owned(),
                    ));
                }
                Ok(self
                    .store
                    .list_file_change_events_by_id(space_id, limit, cursor.map(|cursor| cursor.id))
                    .await?)
            },
            |event| FileChangeEventIdCursor {
                space_id,
                id: event.id,
            },
        )
        .await?;

        self.filter_external_events(space_id, &mut items).await?;
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

        let mut page = shape_file_change_sync_page(batch, request.after_id, limit);
        require_external_snapshot_refresh(self.channel, &mut page);
        page.resync_required |= self
            .filter_external_events(space_id, &mut page.items)
            .await?;
        Ok(page)
    }
}

fn require_external_snapshot_refresh(
    channel: notegate_model::Channel,
    page: &mut FileChangeSyncPage,
) {
    if channel != notegate_model::Channel::Browser {
        // Inspect the raw page before visibility filtering removes revocations.
        page.resync_required |= page.items.iter().any(|event| {
            event.metadata.get("external_access_enabled_changed")
                == Some(&serde_json::Value::Bool(true))
                || event.node_id.is_none()
        });
    }
}

fn redact_subtree_counts(event: &mut notegate_model::FileChangeEvent) {
    // Browser operations may include descendants that external callers cannot see.
    if let Some(metadata) = event.metadata.as_object_mut() {
        for key in [
            "copied_nodes",
            "copied_texts",
            "copied_files",
            "deleted_nodes",
        ] {
            metadata.remove(key);
        }
    }
}

fn event_node_ids(event: &notegate_model::FileChangeEvent) -> impl Iterator<Item = Uuid> + '_ {
    event.node_id.into_iter().chain(
        [
            "parent_node_id",
            "parent_node_id_before",
            "parent_node_id_after",
            "copied_from_node_id",
        ]
        .into_iter()
        .filter_map(|key| {
            event
                .metadata
                .get(key)
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok())
        }),
    )
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
    fn external_history_omits_subtree_counts_without_losing_navigation() {
        let parent = Uuid::new_v4();
        for op in ["item.copy", "item.delete"] {
            let mut row = event(42);
            row.op_type = op.to_owned();
            row.metadata = serde_json::json!({
                "item_kind": "folder",
                "item_name": "public",
                "parent_node_id_after": parent,
                "copied_nodes": 5,
                "copied_texts": 3,
                "copied_files": 1,
                "deleted_nodes": 5,
                "recursive": true,
            });
            let browser_metadata = row.metadata.clone();
            redact_subtree_counts(&mut row);
            for key in [
                "copied_nodes",
                "copied_texts",
                "copied_files",
                "deleted_nodes",
            ] {
                assert!(row.metadata.get(key).is_none());
                assert!(browser_metadata.get(key).is_some());
            }
            assert_eq!(
                row.metadata.get("item_name"),
                Some(&serde_json::json!("public"))
            );
            assert_eq!(
                row.metadata.get("parent_node_id_after"),
                Some(&serde_json::json!(parent))
            );
            assert_eq!(
                row.metadata.get("recursive"),
                Some(&serde_json::json!(true))
            );
        }
    }

    #[test]
    fn external_policy_changes_require_resync_in_both_directions() {
        for channel in [
            notegate_model::Channel::Browser,
            notegate_model::Channel::Mcp,
            notegate_model::Channel::Api,
        ] {
            for enabled in [false, true] {
                let mut row = event(42);
                row.node_id = Some(Uuid::new_v4());
                row.metadata = serde_json::json!({
                    "external_access_enabled_changed": true,
                    "external_access_enabled": enabled,
                });
                let mut page = shape_file_change_sync_page(
                    FileChangeSyncRows {
                        events: vec![row],
                        latest_id: 42,
                        token_valid: true,
                    },
                    Some(41),
                    10,
                );
                require_external_snapshot_refresh(channel, &mut page);
                assert_eq!(
                    page.resync_required,
                    channel != notegate_model::Channel::Browser
                );
                assert_eq!(page.next_after_id, 42);
            }
        }
    }

    #[test]
    fn ordinary_events_do_not_require_resync_but_purged_nodes_do() {
        let mut row = event(42);
        row.node_id = Some(Uuid::new_v4());
        row.metadata = serde_json::json!({"external_access_enabled_changed": false});
        let mut page = shape_file_change_sync_page(
            FileChangeSyncRows {
                events: vec![row],
                latest_id: 42,
                token_valid: true,
            },
            Some(41),
            10,
        );
        require_external_snapshot_refresh(notegate_model::Channel::Mcp, &mut page);
        assert!(!page.resync_required);
        for event in &mut page.items {
            event.node_id = None;
        }
        require_external_snapshot_refresh(notegate_model::Channel::Mcp, &mut page);
        assert!(page.resync_required);
        assert_eq!(page.next_after_id, 42);
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
