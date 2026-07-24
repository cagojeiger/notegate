//! Integration tests for durable file change event capture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, attach_file, space_with_root};
use notegate_core::Error;
use notegate_db::{FilesRepo, MetadataMutationKind, TextMutationKind};
use notegate_model::files::{CopyNode, CreateFolder, MoveNode, StoredContent, WriteTextBody};
use serde_json::json;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

#[tokio::test]
async fn file_tree_mutations_write_file_change_events() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "file-change-events").await?;
    let repo = FilesRepo::new(db.pool.clone());

    let root_rename = repo
        .update_node_metadata(space_id, root_id, Some("/"), None, account)
        .await
        .expect_err("root rename should be rejected even when the name is unchanged");
    assert!(matches!(
        root_rename,
        Error::Conflict(ref message) if message == "cannot rename the root node"
    ));

    let folder = repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "docs".to_owned(),
            },
            account,
        )
        .await?;
    let (node, _) = repo
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    let (file_node, _) = attach_file(&repo, space_id, root_id, "asset.txt", 5, account).await?;
    let (_, written_text) = repo
        .save_text_content(
            space_id,
            node.id,
            &text("hello world"),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    let (_, no_op_text) = repo
        .save_text_content(
            space_id,
            node.id,
            &text("hello world"),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(no_op_text.updated_at, written_text.updated_at);
    let metadata = json!({ "source": "test" });
    let metadata_node = repo
        .replace_node_metadata(
            space_id,
            node.id,
            &metadata,
            account,
            MetadataMutationKind::Replace,
        )
        .await?;
    let no_op_metadata_node = repo
        .replace_node_metadata(
            space_id,
            node.id,
            &metadata,
            account,
            MetadataMutationKind::Replace,
        )
        .await?;
    assert_eq!(no_op_metadata_node.updated_at, metadata_node.updated_at);
    let updated_node = repo
        .update_node_metadata(space_id, node.id, Some("renamed.md"), Some(10), account)
        .await?;
    let no_op_updated_node = repo
        .update_node_metadata(space_id, node.id, Some("renamed.md"), Some(10), account)
        .await?;
    assert_eq!(no_op_updated_node.updated_at, updated_node.updated_at);
    let moved_node = repo
        .move_node(
            space_id,
            &MoveNode {
                node_id: node.id,
                new_parent_node_id: folder.id,
                new_name: None,
                expected_parent_id: Some(root_id),
            },
            account,
        )
        .await?;
    let no_op_moved_node = repo
        .move_node(
            space_id,
            &MoveNode {
                node_id: node.id,
                new_parent_node_id: folder.id,
                new_name: None,
                expected_parent_id: Some(folder.id),
            },
            account,
        )
        .await?;
    assert_eq!(no_op_moved_node.updated_at, moved_node.updated_at);
    let (copied_node, _) = repo
        .copy_node(
            space_id,
            &CopyNode {
                node_id: node.id,
                new_parent_node_id: root_id,
                new_name: "renamed-copy.md".to_owned(),
                recursive: false,
            },
            account,
        )
        .await?;
    repo.soft_delete_node(space_id, node.id, account, false)
        .await?;

    let events = repo
        .list_file_change_events(space_id, None, 20, None)
        .await?;
    let op_types: Vec<_> = events.iter().map(|event| event.op_type.as_str()).collect();
    assert_eq!(
        op_types,
        vec![
            "item.delete",
            "item.copy",
            "item.move",
            "item.update",
            "metadata.replace",
            "text.write",
            "file.create",
            "text.create",
            "folder.create",
        ]
    );
    assert!(events.iter().all(|event| event.space_id == space_id));
    assert!(
        events
            .iter()
            .all(|event| event.actor_account_id == Some(account))
    );
    assert_eq!(events[5].metadata["byte_len_before"], json!(5));
    assert_eq!(events[5].metadata["byte_len_after"], json!(11));
    assert_eq!(events[5].metadata["parent_node_id"], json!(root_id));
    assert_eq!(events[6].node_id, Some(file_node.id));
    assert_eq!(events[6].metadata["byte_len_after"], json!(5));
    assert!(events[6].metadata.get("line_count_after").is_none());
    assert_eq!(events[1].node_id, Some(copied_node.id));
    assert_eq!(events[1].metadata["item_kind"], json!("text"));
    assert_eq!(events[1].metadata["copied_from_node_id"], json!(node.id));
    assert_eq!(
        events[0].metadata["parent_node_id_before"],
        json!(folder.id)
    );
    assert_eq!(events[3].metadata["parent_node_id"], json!(root_id));
    assert_eq!(events[4].metadata["parent_node_id"], json!(root_id));
    assert!(events[5].metadata.get("content_sha256_before").is_none());
    assert!(events[5].metadata.get("content_sha256_after").is_none());

    let file_change_events = repo
        .list_file_change_events(space_id, Some(node.id), 20, None)
        .await?;
    assert_eq!(file_change_events.len(), 6);
    assert!(
        file_change_events
            .iter()
            .all(|event| event.node_id == Some(node.id))
    );

    let baseline = repo.sync_file_change_events(space_id, None, 3).await?;
    assert!(baseline.events.is_empty());
    assert!(baseline.token_valid);
    assert_eq!(baseline.latest_id, events[0].id);

    repo.insert_folder(
        space_id,
        &CreateFolder {
            parent_node_id: root_id,
            name: "after-a".to_owned(),
        },
        account,
    )
    .await?;
    repo.insert_folder(
        space_id,
        &CreateFolder {
            parent_node_id: root_id,
            name: "after-b".to_owned(),
        },
        account,
    )
    .await?;
    let forward = repo
        .sync_file_change_events(space_id, Some(baseline.latest_id), 3)
        .await?;
    assert!(forward.token_valid);
    assert_eq!(forward.events.len(), 2);
    assert!(forward.events[0].id < forward.events[1].id);
    assert!(
        forward
            .events
            .iter()
            .all(|event| event.metadata["parent_node_id"] == json!(root_id))
    );

    let invalid = repo
        .sync_file_change_events(space_id, Some(forward.latest_id + 1000), 3)
        .await?;
    assert!(!invalid.token_valid);
    assert!(invalid.events.is_empty());
    assert_eq!(invalid.latest_id, forward.latest_id);

    db.cleanup().await;
    Ok(())
}
