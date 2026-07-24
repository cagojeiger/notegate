use std::collections::HashMap;

use chrono::{DateTime, Utc};
use notegate_model::{AccountRef as ModelAccountRef, AuditEvent};
use notegate_service::files::FileChangeEvent;
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AccountRef;

/// Audit event history entry returned by `GET /api/v1/me/audit-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub source: String,
    pub op_type: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

impl AuditEventOut {
    pub(crate) fn from_event(event: &AuditEvent, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            source: event.source.clone(),
            op_type: event.op_type.clone(),
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id,
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventListResponse {
    pub events: Vec<AuditEventOut>,
    pub page: crate::page::Page,
}

/// File change event history entry returned by `GET /api/v1/spaces/{space_id}/file-change-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub actor: Option<AccountRef>,
    pub op_type: String,
    pub metadata: Value,
}

impl FileChangeEventOut {
    pub(crate) fn from_event(
        event: &FileChangeEvent,
        refs: &HashMap<Uuid, ModelAccountRef>,
    ) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            space_id: event.space_id,
            node_id: event.node_id,
            actor_account_id: event.actor_account_id,
            actor: event
                .actor_account_id
                .and_then(|id| refs.get(&id).map(AccountRef::from)),
            op_type: event.op_type.clone(),
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventListResponse {
    pub events: Vec<FileChangeEventOut>,
    pub page: crate::page::Page,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeDeltaOut {
    pub id: i64,
    pub node_id: Option<Uuid>,
    pub op_type: String,
    pub item_kind: Option<String>,
    pub affected_parent_ids: Vec<Uuid>,
    pub parent_scope_known: bool,
    pub path_changed: bool,
    pub subtree_changed: bool,
}

impl FileChangeDeltaOut {
    pub(crate) fn from_event(event: &FileChangeEvent) -> Self {
        let item_kind = metadata_string(&event.metadata, "item_kind");
        let affected_parent_ids = [
            metadata_uuid(&event.metadata, "parent_node_id_before"),
            metadata_uuid(&event.metadata, "parent_node_id_after"),
            metadata_uuid(&event.metadata, "parent_node_id"),
        ]
        .into_iter()
        .flatten()
        .fold(Vec::new(), |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        });
        let parent_scope_known = [
            "parent_node_id",
            "parent_node_id_before",
            "parent_node_id_after",
        ]
        .into_iter()
        .any(|key| event.metadata.get(key).is_some());
        let path_changed = matches!(
            event.op_type.as_str(),
            "folder.create"
                | "text.create"
                | "file.create"
                | "item.copy"
                | "item.move"
                | "item.delete"
        ) || (event.op_type == "item.update"
            && metadata_bool(&event.metadata, "name_changed"));
        let subtree_changed = item_kind.as_deref() == Some("folder")
            && (event.op_type == "item.move"
                || (event.op_type == "item.update"
                    && metadata_bool(&event.metadata, "name_changed"))
                || (event.op_type == "item.delete" && metadata_bool(&event.metadata, "recursive")));

        Self {
            id: event.id,
            node_id: event.node_id,
            op_type: event.op_type.clone(),
            item_kind,
            affected_parent_ids,
            parent_scope_known,
            path_changed,
            subtree_changed,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeSyncResponse {
    pub changes: Vec<FileChangeDeltaOut>,
    pub next_after_id: i64,
    pub has_more: bool,
    pub resync_required: bool,
}

fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata.get(key)?.as_str().map(str::to_owned)
}

fn metadata_uuid(metadata: &Value, key: &str) -> Option<Uuid> {
    metadata.get(key)?.as_str()?.parse().ok()
}

fn metadata_bool(metadata: &Value, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn delta_maps_move_parents_and_subtree_scope() {
        let before = Uuid::new_v4();
        let after = Uuid::new_v4();
        let delta = FileChangeDeltaOut::from_event(&event(
            "item.move",
            json!({
                "item_kind": "folder",
                "parent_node_id_before": before,
                "parent_node_id_after": after,
                "name_changed": false,
            }),
        ));

        assert_eq!(delta.affected_parent_ids, vec![before, after]);
        assert!(delta.parent_scope_known);
        assert!(delta.path_changed);
        assert!(delta.subtree_changed);
    }

    #[test]
    fn delta_maps_leaf_parent_without_subtree_refresh() {
        let parent = Uuid::new_v4();
        let delta = FileChangeDeltaOut::from_event(&event(
            "text.write",
            json!({
                "item_kind": "text",
                "parent_node_id": parent,
            }),
        ));

        assert_eq!(delta.affected_parent_ids, vec![parent]);
        assert!(delta.parent_scope_known);
        assert!(!delta.path_changed);
        assert!(!delta.subtree_changed);
    }

    #[test]
    fn delta_marks_create_as_path_change_without_existing_subtree_impact() {
        let parent = Uuid::new_v4();
        let delta = FileChangeDeltaOut::from_event(&event(
            "folder.create",
            json!({
                "item_kind": "folder",
                "parent_node_id": parent,
            }),
        ));

        assert!(delta.path_changed);
        assert!(!delta.subtree_changed);
    }

    #[test]
    fn delta_marks_historical_event_without_parent_context() {
        let delta =
            FileChangeDeltaOut::from_event(&event("text.write", json!({ "item_kind": "text" })));

        assert!(delta.affected_parent_ids.is_empty());
        assert!(!delta.parent_scope_known);
    }

    fn event(op_type: &str, metadata: Value) -> FileChangeEvent {
        FileChangeEvent {
            id: 1,
            created_at: Utc::now(),
            space_id: Uuid::new_v4(),
            node_id: Some(Uuid::new_v4()),
            actor_account_id: Some(Uuid::new_v4()),
            op_type: op_type.to_owned(),
            metadata,
        }
    }
}
