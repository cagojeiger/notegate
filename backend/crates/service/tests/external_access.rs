#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{FilesRepo, SpaceRepo};
use notegate_model::{AccountKind, Channel};
use notegate_service::files::{
    BatchChildrenRequest, BatchChildrenResult, ChildrenRequest, CopyNode, CreateFolder, CreateText,
    FilesService, ListFileChangeEventsById, ListNodesRequest, MoveNode, NodeListSort, ReadText,
    SyncFileChanges, UpdateNodeExternalAccessPolicy,
};

#[tokio::test]
async fn collection_pages_exclude_private_nodes_before_pagination()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let owner = insert_user_account(&db.pool, "list-owner", "list-owner@example.test").await?;
    let (space, root) = setup_space(&SpaceRepo::new(db.pool.clone()), owner, "external-list").await;
    let hidden = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "a-private.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id: hidden,
                enabled: false,
            },
        )
        .await?;
    let visible = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "z-public.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;

    // A baseline must advance even when the latest event will be filtered out.
    for enabled in [true, false, true, false] {
        let baseline = files
            .sync_file_changes(
                owner,
                space,
                SyncFileChanges {
                    after_id: None,
                    limit: Some(1),
                },
            )
            .await?;
        files
            .update_node_external_access_policy(
                AccountKind::User,
                owner,
                space,
                UpdateNodeExternalAccessPolicy {
                    node_id: hidden,
                    enabled,
                },
            )
            .await?;
        for channel in [Channel::Api, Channel::Mcp, Channel::Browser] {
            let scoped = files.for_channel(channel);
            let page = scoped
                .sync_file_changes(
                    owner,
                    space,
                    SyncFileChanges {
                        after_id: Some(baseline.next_after_id),
                        limit: Some(1),
                    },
                )
                .await?;
            assert!(page.next_after_id > baseline.next_after_id);
            assert_eq!(page.resync_required, channel != Channel::Browser);
            if !enabled && channel != Channel::Browser {
                assert!(page.items.is_empty());
            }
            let current = scoped
                .sync_file_changes(
                    owner,
                    space,
                    SyncFileChanges {
                        after_id: None,
                        limit: Some(1),
                    },
                )
                .await?;
            assert_eq!(current.next_after_id, page.next_after_id);
            let caught_up = scoped
                .sync_file_changes(
                    owner,
                    space,
                    SyncFileChanges {
                        after_id: Some(page.next_after_id),
                        limit: Some(1),
                    },
                )
                .await?;
            assert!(!caught_up.resync_required);
            assert!(caught_up.items.is_empty());
        }
    }

    for channel in [Channel::Api, Channel::Mcp] {
        let external = files.for_channel(channel);
        let page = external
            .canonical_children(
                owner,
                space,
                root,
                ChildrenRequest {
                    limit: Some(1),
                    cursor: None,
                },
            )
            .await?;
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items.first().map(|item| item.node.id), Some(visible));
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
        let compact = external
            .children(
                owner,
                space,
                root,
                ChildrenRequest {
                    limit: Some(1),
                    cursor: None,
                },
            )
            .await?;
        assert_eq!(
            compact.items.first().map(|item| item.node.id),
            Some(visible)
        );
        assert!(!compact.has_more);
        let history = external
            .list_file_change_events_by_id(
                owner,
                space,
                ListFileChangeEventsById {
                    limit: Some(100),
                    cursor: None,
                },
            )
            .await?;
        assert!(!history.items.is_empty());
        assert!(
            history
                .items
                .iter()
                .all(|event| event.node_id == Some(visible))
        );
    }
    let browser = files
        .canonical_children(
            owner,
            space,
            root,
            ChildrenRequest {
                limit: Some(1),
                cursor: None,
            },
        )
        .await?;
    assert_eq!(browser.items.first().map(|item| item.node.id), Some(hidden));
    assert!(browser.has_more);
    Ok(())
}

#[tokio::test]
async fn mutation_transactions_reject_protected_subtrees() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(repo.clone());
    let owner = insert_user_account(&db.pool, "move-owner", "move-owner@example.test").await?;
    let (space, root) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "external-subtree").await;
    let folder = files
        .create_folder(
            owner,
            space,
            CreateFolder {
                parent_node_id: root,
                name: "folder".to_owned(),
            },
        )
        .await?
        .node
        .id;
    let hidden = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: folder,
                name: "private.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id: hidden,
                enabled: false,
            },
        )
        .await?;
    let external = repo.clone().with_external_access_only(true);
    // Call the repository directly: protection must not depend on a prior service read.
    assert!(matches!(
        external
            .copy_node(
                space,
                &CopyNode {
                    node_id: folder,
                    new_parent_node_id: root,
                    new_name: "copy".to_owned(),
                    recursive: true,
                },
                owner
            )
            .await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(matches!(
        external
            .move_node(
                space,
                &MoveNode {
                    node_id: folder,
                    new_parent_node_id: root,
                    new_name: Some("renamed".to_owned()),
                    expected_parent_id: None,
                },
                owner
            )
            .await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(matches!(
        external.soft_delete_node(space, folder, owner, true).await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(repo.find_node(space, hidden).await?.is_some());
    assert_eq!(
        repo.find_node(space, folder).await?.map(|node| node.name),
        Some("folder".to_owned())
    );
    assert!(repo.resolve_path(space, "/copy").await?.is_none());

    let copied = files
        .copy_node(
            owner,
            space,
            CopyNode {
                node_id: folder,
                new_parent_node_id: root,
                new_name: "browser-copy".to_owned(),
                recursive: true,
            },
        )
        .await?;
    repo.soft_delete_node(space, copied.node.node.id, owner, true)
        .await?;
    for channel in [Channel::Browser, Channel::Api, Channel::Mcp] {
        let history = files
            .for_channel(channel)
            .list_file_change_events_by_id(
                owner,
                space,
                ListFileChangeEventsById {
                    limit: Some(100),
                    cursor: None,
                },
            )
            .await?;
        for (op, count_key) in [
            ("item.copy", "copied_nodes"),
            ("item.delete", "deleted_nodes"),
        ] {
            let event = history
                .items
                .iter()
                .find(|event| event.op_type == op)
                .expect("accessible parent event remains visible after soft deletion");
            assert_eq!(
                event.metadata.get(count_key).is_some(),
                channel == Channel::Browser
            );
        }
    }
    // An enabled descendant is still private while its ancestor is disabled.
    for (node_id, enabled) in [(hidden, true), (folder, false)] {
        files
            .update_node_external_access_policy(
                AccountKind::User,
                owner,
                space,
                UpdateNodeExternalAccessPolicy { node_id, enabled },
            )
            .await?;
    }
    assert!(
        repo.find_node(space, hidden)
            .await?
            .unwrap()
            .external_access_enabled
    );
    for channel in [Channel::Api, Channel::Mcp] {
        let scoped = files.for_channel(channel);
        assert!(matches!(
            scoped.stat(owner, space, hidden).await,
            Err(notegate_service::ServiceError::NotFound(_))
        ));
        assert!(matches!(
            scoped.reveal_node(owner, space, hidden).await,
            Err(notegate_service::ServiceError::NotFound(_))
        ));
        let current_hash = repo
            .text_stats(space, hidden)
            .await?
            .unwrap()
            .content_sha256;
        for if_none_match_sha256 in [None, Some(current_hash)] {
            assert!(matches!(
                scoped
                    .read_text(
                        owner,
                        space,
                        ReadText {
                            node_id: hidden,
                            start_line: None,
                            max_lines: None,
                            max_bytes: None,
                            if_none_match_sha256,
                        }
                    )
                    .await,
                Err(notegate_service::ServiceError::NotFound(_))
            ));
        }
        let batch = scoped
            .batch_children(
                owner,
                space,
                BatchChildrenRequest {
                    parent_node_ids: vec![folder],
                    limit: Some(1),
                },
            )
            .await?;
        assert!(
            matches!(batch.as_slice(), [BatchChildrenResult::NotFound { parent_node_id }] if *parent_node_id == folder)
        );
        assert!(matches!(
            scoped
                .create_text(
                    owner,
                    space,
                    CreateText {
                        parent_node_id: folder,
                        name: "blocked.md".to_owned(),
                    }
                )
                .await,
            Err(notegate_service::ServiceError::NotFound(_))
        ));
        let list = scoped
            .list_nodes(
                owner,
                space,
                ListNodesRequest {
                    kind: None,
                    sort: NodeListSort::NameAsc,
                    limit: Some(100),
                    cursor: None,
                },
            )
            .await?;
        assert!(
            list.items
                .iter()
                .all(|item| item.node.id != hidden && item.node.id != folder)
        );
    }
    assert!(matches!(
        external.soft_delete_node(space, hidden, owner, false).await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(matches!(
        external
            .move_node(
                space,
                &MoveNode {
                    node_id: hidden,
                    new_parent_node_id: root,
                    new_name: None,
                    expected_parent_id: None,
                },
                owner
            )
            .await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(matches!(
        external
            .copy_node(
                space,
                &CopyNode {
                    node_id: hidden,
                    new_parent_node_id: root,
                    new_name: "escaped.md".to_owned(),
                    recursive: false,
                },
                owner
            )
            .await,
        Err(notegate_core::Error::NotFound(_))
    ));
    let outside = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "outside.md".to_owned(),
            },
        )
        .await?
        .node
        .node
        .id;
    assert!(matches!(
        external
            .move_node(
                space,
                &MoveNode {
                    node_id: outside,
                    new_parent_node_id: folder,
                    new_name: None,
                    expected_parent_id: None,
                },
                owner
            )
            .await,
        Err(notegate_core::Error::NotFound(_))
    ));
    assert!(
        files
            .stat(owner, space, hidden)
            .await?
            .node
            .external_access_enabled
    );
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id: folder,
                enabled: true,
            },
        )
        .await?;
    for channel in [Channel::Api, Channel::Mcp] {
        assert!(
            files
                .for_channel(channel)
                .stat(owner, space, hidden)
                .await?
                .node
                .external_access_enabled
        );
    }
    repo.move_node(
        space,
        &MoveNode {
            node_id: folder,
            new_parent_node_id: root,
            new_name: Some("browser-renamed".to_owned()),
            expected_parent_id: None,
        },
        owner,
    )
    .await?;
    Ok(())
}
