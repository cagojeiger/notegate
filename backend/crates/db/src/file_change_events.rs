//! Typed constructors for `file_change_events` rows: one function per event
//! kind builds its `op_type` + `metadata` (see `docs/spec/event-logging.md`
//! for the allowlist), so the file-tree commands never inline strings or
//! `json!` payloads — they only call a constructor here.
//!
//! To add a new event type: add a `..._payload` function that returns the
//! `(op_type, metadata)` pair, a thin `pub(crate) async fn` wrapper that
//! forwards it to `event`, and a unit test asserting the payload shape.

use crate::file_change_event_repo::{NewFileChangeEvent, insert_file_change_event};
use crate::files_repo::{MetadataMutationKind, TextMutationKind};
use notegate_core::Result;
use notegate_model::files::CopyCounts;
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileChangeContext {
    actor_account_id: Uuid,
    space_id: Uuid,
}

pub(crate) fn context(actor_account_id: Uuid, space_id: Uuid) -> FileChangeContext {
    FileChangeContext {
        actor_account_id,
        space_id,
    }
}

async fn event(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Option<Uuid>,
    op_type: &'static str,
    metadata: Value,
) -> Result<()> {
    insert_file_change_event(
        tx,
        NewFileChangeEvent {
            space_id: ctx.space_id,
            node_id,
            actor_account_id: Some(ctx.actor_account_id),
            op_type,
            metadata,
        },
    )
    .await
}

fn folder_created_payload(item_name: &str, parent_node_id: Uuid) -> (&'static str, Value) {
    (
        "folder.create",
        json!({
            "item_kind": "folder",
            "item_name": item_name,
            "parent_node_id": parent_node_id,
        }),
    )
}

pub(crate) async fn folder_created(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    item_name: &str,
    parent_node_id: Uuid,
) -> Result<()> {
    let (op_type, metadata) = folder_created_payload(item_name, parent_node_id);
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn text_created_payload(
    item_name: &str,
    parent_node_id: Uuid,
    byte_len_after: i64,
    line_count_after: i32,
) -> (&'static str, Value) {
    (
        "text.create",
        json!({
            "item_kind": "text",
            "item_name": item_name,
            "parent_node_id": parent_node_id,
            "byte_len_after": byte_len_after,
            "line_count_after": line_count_after,
        }),
    )
}

pub(crate) async fn text_created(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    item_name: &str,
    parent_node_id: Uuid,
    byte_len_after: i64,
    line_count_after: i32,
) -> Result<()> {
    let (op_type, metadata) =
        text_created_payload(item_name, parent_node_id, byte_len_after, line_count_after);
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn file_created_payload(
    item_name: &str,
    parent_node_id: Uuid,
    byte_len_after: i64,
) -> (&'static str, Value) {
    (
        "file.create",
        json!({
            "item_kind": "file",
            "item_name": item_name,
            "parent_node_id": parent_node_id,
            "byte_len_after": byte_len_after,
        }),
    )
}

pub(crate) async fn file_created(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    item_name: &str,
    parent_node_id: Uuid,
    byte_len_after: i64,
) -> Result<()> {
    let (op_type, metadata) = file_created_payload(item_name, parent_node_id, byte_len_after);
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn text_saved_payload(
    kind: TextMutationKind,
    item_name: &str,
    parent_node_id: Option<Uuid>,
    byte_len_before: i64,
    byte_len_after: i64,
    line_count_before: i32,
    line_count_after: i32,
) -> (&'static str, Value) {
    (
        kind.op_type(),
        json!({
            "item_kind": "text",
            "item_name": item_name,
            "parent_node_id": parent_node_id,
            "byte_len_before": byte_len_before,
            "byte_len_after": byte_len_after,
            "line_count_before": line_count_before,
            "line_count_after": line_count_after,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn text_saved(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    item_name: &str,
    parent_node_id: Option<Uuid>,
    kind: TextMutationKind,
    byte_len_before: i64,
    byte_len_after: i64,
    line_count_before: i32,
    line_count_after: i32,
) -> Result<()> {
    let (op_type, metadata) = text_saved_payload(
        kind,
        item_name,
        parent_node_id,
        byte_len_before,
        byte_len_after,
        line_count_before,
        line_count_after,
    );
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn node_metadata_replaced_payload(
    kind: MetadataMutationKind,
    item_kind: &str,
    item_name: &str,
    parent_node_id: Option<Uuid>,
) -> (&'static str, Value) {
    (
        kind.op_type(),
        json!({
            "item_kind": item_kind,
            "item_name": item_name,
            "parent_node_id": parent_node_id,
        }),
    )
}

pub(crate) async fn node_metadata_replaced(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    kind: MetadataMutationKind,
    item_kind: &str,
    item_name: &str,
    parent_node_id: Option<Uuid>,
) -> Result<()> {
    let (op_type, metadata) =
        node_metadata_replaced_payload(kind, item_kind, item_name, parent_node_id);
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn node_updated_payload(
    item_kind: &str,
    item_name: &str,
    parent_node_id: Option<Uuid>,
    name_changed: bool,
    sort_order_changed: bool,
) -> (&'static str, Value) {
    (
        "item.update",
        json!({
            "item_kind": item_kind,
            "item_name": item_name,
            "parent_node_id": parent_node_id,
            "name_changed": name_changed,
            "sort_order_changed": sort_order_changed,
        }),
    )
}

pub(crate) struct NodeUpdated<'a> {
    pub item_kind: &'a str,
    pub item_name: &'a str,
    pub parent_node_id: Option<Uuid>,
    pub name_changed: bool,
    pub sort_order_changed: bool,
}

pub(crate) async fn node_updated(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    updated: NodeUpdated<'_>,
) -> Result<()> {
    let (op_type, metadata) = node_updated_payload(
        updated.item_kind,
        updated.item_name,
        updated.parent_node_id,
        updated.name_changed,
        updated.sort_order_changed,
    );
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn node_moved_payload(
    item_kind: &str,
    item_name: &str,
    parent_node_id_before: Option<Uuid>,
    parent_node_id_after: Uuid,
    name_changed: bool,
) -> (&'static str, Value) {
    (
        "item.move",
        json!({
            "item_kind": item_kind,
            "item_name": item_name,
            "parent_node_id_before": parent_node_id_before,
            "parent_node_id_after": parent_node_id_after,
            "name_changed": name_changed,
        }),
    )
}

pub(crate) struct NodeMoved<'a> {
    pub item_kind: &'a str,
    pub item_name: &'a str,
    pub parent_node_id_before: Option<Uuid>,
    pub parent_node_id_after: Uuid,
    pub name_changed: bool,
}

pub(crate) async fn node_moved(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    moved: NodeMoved<'_>,
) -> Result<()> {
    let (op_type, metadata) = node_moved_payload(
        moved.item_kind,
        moved.item_name,
        moved.parent_node_id_before,
        moved.parent_node_id_after,
        moved.name_changed,
    );
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

fn node_copied_payload(
    item_kind: &str,
    item_name: &str,
    copied_from_node_id: Uuid,
    parent_node_id_after: Uuid,
    counts: CopyCounts,
    recursive: bool,
) -> (&'static str, Value) {
    (
        "item.copy",
        json!({
            "item_kind": item_kind,
            "item_name": item_name,
            "copied_from_node_id": copied_from_node_id,
            "parent_node_id_after": parent_node_id_after,
            "copied_nodes": counts.nodes,
            "copied_texts": counts.texts,
            "copied_files": counts.files,
            "recursive": recursive,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn node_copied(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    new_node_id: Uuid,
    item_kind: &str,
    item_name: &str,
    copied_from_node_id: Uuid,
    parent_node_id_after: Uuid,
    counts: CopyCounts,
    recursive: bool,
) -> Result<()> {
    let (op_type, metadata) = node_copied_payload(
        item_kind,
        item_name,
        copied_from_node_id,
        parent_node_id_after,
        counts,
        recursive,
    );
    event(tx, ctx, Some(new_node_id), op_type, metadata).await
}

fn node_deleted_payload(
    item_kind: &str,
    item_name: &str,
    parent_node_id_before: Option<Uuid>,
    deleted_nodes: usize,
    recursive: bool,
) -> (&'static str, Value) {
    (
        "item.delete",
        json!({
            "item_kind": item_kind,
            "item_name": item_name,
            "parent_node_id_before": parent_node_id_before,
            "deleted_nodes": deleted_nodes,
            "recursive": recursive,
        }),
    )
}

pub(crate) struct NodeDeleted<'a> {
    pub item_kind: &'a str,
    pub item_name: &'a str,
    pub parent_node_id_before: Option<Uuid>,
    pub deleted_nodes: usize,
    pub recursive: bool,
}

pub(crate) async fn node_deleted(
    tx: &mut PgConnection,
    ctx: FileChangeContext,
    node_id: Uuid,
    deleted: NodeDeleted<'_>,
) -> Result<()> {
    let (op_type, metadata) = node_deleted_payload(
        deleted.item_kind,
        deleted.item_name,
        deleted.parent_node_id_before,
        deleted.deleted_nodes,
        deleted.recursive,
    );
    event(tx, ctx, Some(node_id), op_type, metadata).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_created_builds_expected_payload() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = folder_created_payload("notes", parent);
        assert_eq!(op_type, "folder.create");
        assert_eq!(
            metadata,
            json!({ "item_kind": "folder", "item_name": "notes", "parent_node_id": parent })
        );
    }

    #[test]
    fn text_created_builds_expected_payload() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = text_created_payload("draft.md", parent, 11, 2);
        assert_eq!(op_type, "text.create");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "text",
                "item_name": "draft.md",
                "parent_node_id": parent,
                "byte_len_after": 11,
                "line_count_after": 2,
            })
        );
    }

    #[test]
    fn file_created_builds_expected_payload() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = file_created_payload("image.png", parent, 5);
        assert_eq!(op_type, "file.create");
        assert_eq!(
            metadata,
            json!({ "item_kind": "file", "item_name": "image.png", "parent_node_id": parent, "byte_len_after": 5 })
        );
    }

    #[test]
    fn text_saved_uses_mutation_kind_op_type() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = text_saved_payload(
            TextMutationKind::Append,
            "draft.md",
            Some(parent),
            5,
            11,
            1,
            2,
        );
        assert_eq!(op_type, "text.append");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "text",
                "item_name": "draft.md",
                "parent_node_id": parent,
                "byte_len_before": 5,
                "byte_len_after": 11,
                "line_count_before": 1,
                "line_count_after": 2,
            })
        );
    }

    #[test]
    fn node_metadata_replaced_uses_mutation_kind_op_type() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = node_metadata_replaced_payload(
            MetadataMutationKind::Patch,
            "text",
            "draft.md",
            Some(parent),
        );
        assert_eq!(op_type, "metadata.patch");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "text",
                "item_name": "draft.md",
                "parent_node_id": parent,
            })
        );
    }

    #[test]
    fn node_updated_builds_expected_payload() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) =
            node_updated_payload("folder", "renamed", Some(parent), true, false);
        assert_eq!(op_type, "item.update");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "folder",
                "item_name": "renamed",
                "parent_node_id": parent,
                "name_changed": true,
                "sort_order_changed": false,
            })
        );
    }

    #[test]
    fn node_moved_builds_expected_payload() {
        let before = Uuid::new_v4();
        let after = Uuid::new_v4();
        let (op_type, metadata) = node_moved_payload("text", "moved.md", Some(before), after, true);
        assert_eq!(op_type, "item.move");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "text",
                "item_name": "moved.md",
                "parent_node_id_before": before,
                "parent_node_id_after": after,
                "name_changed": true,
            })
        );
    }

    #[test]
    fn node_copied_builds_expected_payload() {
        let source = Uuid::new_v4();
        let dest_parent = Uuid::new_v4();
        let counts = CopyCounts {
            nodes: 3,
            texts: 2,
            files: 1,
        };
        let (op_type, metadata) =
            node_copied_payload("folder", "copy", source, dest_parent, counts, true);
        assert_eq!(op_type, "item.copy");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "folder",
                "item_name": "copy",
                "copied_from_node_id": source,
                "parent_node_id_after": dest_parent,
                "copied_nodes": 3,
                "copied_texts": 2,
                "copied_files": 1,
                "recursive": true,
            })
        );
    }

    #[test]
    fn node_deleted_builds_expected_payload() {
        let parent = Uuid::new_v4();
        let (op_type, metadata) = node_deleted_payload("file", "old.pdf", Some(parent), 4, true);
        assert_eq!(op_type, "item.delete");
        assert_eq!(
            metadata,
            json!({
                "item_kind": "file",
                "item_name": "old.pdf",
                "parent_node_id_before": parent,
                "deleted_nodes": 4,
                "recursive": true,
            })
        );
    }
}
