use crate::file_change_event_repo::{NewFileChangeEvent, insert_file_change_event};
use notegate_core::Result;
use notegate_model::files::{CopyCounts, StoredContent};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

pub(crate) async fn record_item_created(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    parent_node_id: Uuid,
    item_kind: &str,
    byte_len_after: Option<i64>,
    line_count_after: Option<i32>,
) -> Result<()> {
    let mut metadata = json!({
        "item_kind": item_kind,
        "parent_node_id": parent_node_id,
    });
    if let Some(byte_len) = byte_len_after {
        metadata["byte_len_after"] = json!(byte_len);
    }
    if let Some(line_count) = line_count_after {
        metadata["line_count_after"] = json!(line_count);
    }

    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            match item_kind {
                "folder" => "folder.create",
                "text" => "text.create",
                "file" => "file.create",
                _ => "item.create",
            },
            metadata,
        ),
    )
    .await
}

pub(crate) async fn record_text_changed(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    op_type: &'static str,
    before: ContentMetrics,
    after: ContentMetrics,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            op_type,
            json!({
                "item_kind": "text",
                "byte_len_before": before.byte_len,
                "byte_len_after": after.byte_len,
                "line_count_before": before.line_count,
                "line_count_after": after.line_count,
            }),
        ),
    )
    .await
}

pub(crate) async fn record_metadata_changed(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    item_kind: &str,
    op_type: &'static str,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            op_type,
            json!({ "item_kind": item_kind }),
        ),
    )
    .await
}

pub(crate) async fn record_item_updated(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    item_kind: &str,
    name_changed: bool,
    sort_order_changed: bool,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            "item.update",
            json!({
                "item_kind": item_kind,
                "name_changed": name_changed,
                "sort_order_changed": sort_order_changed,
            }),
        ),
    )
    .await
}

pub(crate) async fn record_item_moved(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    item_kind: &str,
    parent_node_id_before: Option<Uuid>,
    parent_node_id_after: Uuid,
    name_changed: bool,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            "item.move",
            json!({
                "item_kind": item_kind,
                "parent_node_id_before": parent_node_id_before,
                "parent_node_id_after": parent_node_id_after,
                "name_changed": name_changed,
            }),
        ),
    )
    .await
}

pub(crate) async fn record_item_copied(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    new_node_id: Uuid,
    item_kind: &str,
    source_node_id: Uuid,
    parent_node_id_after: Uuid,
    copied: CopyCounts,
    recursive: bool,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(new_node_id),
            "item.copy",
            json!({
                "item_kind": item_kind,
                "copied_from_node_id": source_node_id,
                "parent_node_id_after": parent_node_id_after,
                "copied_nodes": copied.nodes,
                "copied_texts": copied.texts,
                "copied_files": copied.files,
                "recursive": recursive,
            }),
        ),
    )
    .await
}

pub(crate) async fn record_item_deleted(
    tx: &mut PgConnection,
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    item_kind: &str,
    deleted_nodes: usize,
    recursive: bool,
) -> Result<()> {
    insert_file_change_event(
        tx,
        event(
            actor_account_id,
            space_id,
            Some(node_id),
            "item.delete",
            json!({
                "item_kind": item_kind,
                "deleted_nodes": deleted_nodes,
                "recursive": recursive,
            }),
        ),
    )
    .await
}

#[derive(Debug, Clone)]
pub(crate) struct ContentMetrics {
    pub byte_len: i64,
    pub line_count: i32,
}

impl ContentMetrics {
    pub(crate) fn new(byte_len: i64, line_count: i32) -> Self {
        Self {
            byte_len,
            line_count,
        }
    }

    pub(crate) fn from_text(content: &StoredContent) -> Self {
        Self::new(content.byte_len, content.line_count)
    }
}

fn event(
    actor_account_id: Uuid,
    space_id: Uuid,
    node_id: Option<Uuid>,
    op_type: &'static str,
    metadata: serde_json::Value,
) -> NewFileChangeEvent {
    NewFileChangeEvent {
        space_id,
        node_id,
        actor_account_id: Some(actor_account_id),
        op_type,
        metadata,
    }
}
