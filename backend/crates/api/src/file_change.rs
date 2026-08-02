//! Transport-independent derived facts for one file change event.

use notegate_service::files::FileChangeEvent;
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileChangeImpact {
    pub item_kind: Option<String>,
    pub affected_parent_ids: Vec<Uuid>,
    pub parent_scope_known: bool,
    pub path_changed: bool,
    pub subtree_changed: bool,
    pub write_lock_changed: bool,
}

impl FileChangeImpact {
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
        let write_lock_changed =
            event.op_type == "item.update" && metadata_bool(&event.metadata, "write_lock_changed");

        Self {
            item_kind,
            affected_parent_ids,
            parent_scope_known,
            path_changed,
            subtree_changed,
            write_lock_changed,
        }
    }
}

fn metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata.get(key)?.as_str().map(str::to_owned)
}

fn metadata_uuid(metadata: &serde_json::Value, key: &str) -> Option<Uuid> {
    metadata.get(key)?.as_str()?.parse().ok()
}

fn metadata_bool(metadata: &serde_json::Value, key: &str) -> bool {
    metadata
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
