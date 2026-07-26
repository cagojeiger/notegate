#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{Duration, Utc};
use common::{TestDb, space_with_root};
use notegate_core::Error;
use notegate_core::limits;
use notegate_db::{FilesRepo, SpaceRepo, TextMutationKind};
use notegate_model::FileEncryptionMode;
use notegate_model::files::{
    BeginObjectUpload, CopyNode, CreateFolder, MoveNode, NodeListCursor, NodeListSort,
    StoredContent, UpdateNode, WriteTextBody,
};
use uuid::Uuid;

fn assert_not_found<T: std::fmt::Debug>(result: Result<T, Error>) {
    match result {
        Err(Error::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

fn assert_path_too_long<T: std::fmt::Debug>(result: Result<T, Error>) {
    match result {
        Err(Error::Validation(message)) => {
            assert!(
                message.contains("path is too long"),
                "expected path length error, got {message:?}"
            );
        }
        other => panic!("expected path length error, got {other:?}"),
    }
}

fn content() -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain("hello".to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: 5,
        line_count: 1,
    }
}

async fn insert_folder(
    repo: &FilesRepo,
    space_id: Uuid,
    parent_node_id: Uuid,
    name: impl Into<String>,
    account_id: Uuid,
) -> Result<notegate_model::Node, Error> {
    repo.insert_folder(
        space_id,
        &CreateFolder {
            parent_node_id,
            name: name.into(),
        },
        account_id,
    )
    .await
}

async fn long_valid_parent(
    repo: &FilesRepo,
    space_id: Uuid,
    root_id: Uuid,
    account_id: Uuid,
) -> Result<Uuid, Error> {
    let first = insert_folder(
        repo,
        space_id,
        root_id,
        "가".repeat(limits::TEXT_NAME_MAX_LEN),
        account_id,
    )
    .await?;
    let second = insert_folder(
        repo,
        space_id,
        first.id,
        "나".repeat(limits::TEXT_NAME_MAX_LEN),
        account_id,
    )
    .await?;
    Ok(second.id)
}

#[tokio::test]
async fn create_enforces_derived_path_byte_limit_in_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "createpathlimit").await?;
    let repo = FilesRepo::new(db.pool.clone());

    let mut parent_id = root_id;
    let segment = "a".repeat(limits::TEXT_NAME_MAX_LEN);
    for _ in 0..limits::MAX_PATH_DEPTH {
        parent_id = insert_folder(&repo, space_id, parent_id, segment.as_str(), account)
            .await?
            .id;
    }
    let boundary_path = repo
        .node_path(space_id, parent_id)
        .await?
        .expect("boundary node path");
    assert_eq!(boundary_path.len(), limits::MAX_PATH_LEN);
    match insert_folder(&repo, space_id, parent_id, "deeper", account).await {
        Err(Error::Validation(message)) => assert_eq!(message, "path is too deep"),
        other => panic!("expected path depth error, got {other:?}"),
    }

    let long_parent_id = long_valid_parent(&repo, space_id, root_id, account).await?;
    assert_path_too_long(
        insert_folder(&repo, space_id, long_parent_id, "다".repeat(45), account).await,
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_enforces_fanout_in_transaction() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "createfanout").await?;
    let repo = FilesRepo::with_limits(
        db.pool.clone(),
        limits::Limits {
            folder_max_children: 1,
            ..limits::Limits::default()
        },
    );

    insert_folder(&repo, space_id, root_id, "first", account).await?;
    match insert_folder(&repo, space_id, root_id, "second", account).await {
        Err(Error::Conflict(message)) => {
            assert!(message.contains("maximum of 1 live children"));
        }
        other => panic!("expected fanout error, got {other:?}"),
    }

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rename_move_and_copy_enforce_descendant_path_byte_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "mutatepathlimit").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let long_parent_id = long_valid_parent(&repo, space_id, root_id, account).await?;

    let renamed = insert_folder(&repo, space_id, long_parent_id, "short", account).await?;
    insert_folder(&repo, space_id, renamed.id, "다".repeat(30), account).await?;
    assert_path_too_long(
        repo.update_node(
            space_id,
            &UpdateNode {
                node_id: renamed.id,
                name: Some("라".repeat(40)),
                sort_order: None,
            },
            account,
        )
        .await,
    );

    let source = insert_folder(&repo, space_id, root_id, "source", account).await?;
    insert_folder(
        &repo,
        space_id,
        source.id,
        "마".repeat(limits::TEXT_NAME_MAX_LEN),
        account,
    )
    .await?;

    assert_path_too_long(
        repo.move_node(
            space_id,
            &MoveNode {
                node_id: source.id,
                new_parent_node_id: long_parent_id,
                new_name: None,
                expected_parent_id: Some(root_id),
            },
            account,
        )
        .await,
    );
    assert_path_too_long(
        repo.copy_node(
            space_id,
            &CopyNode {
                node_id: source.id,
                new_parent_node_id: long_parent_id,
                new_name: "copy".to_owned(),
                recursive: true,
            },
            account,
        )
        .await,
    );

    let unchanged = repo
        .find_node(space_id, source.id)
        .await?
        .expect("source remains live");
    assert_eq!(unchanged.parent_id, Some(root_id));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn upload_attach_rechecks_path_after_parent_moves() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "uploadpathlimit").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let long_parent_id = long_valid_parent(&repo, space_id, root_id, account).await?;
    let staging = insert_folder(&repo, space_id, root_id, "staging", account).await?;
    let upload_id = Uuid::new_v4();
    let upload_name = "마".repeat(45);

    repo.insert_object_upload(
        upload_id,
        &format!("objects/{upload_id}"),
        space_id,
        account,
        &BeginObjectUpload {
            parent_node_id: staging.id,
            name: upload_name,
            byte_len: 1,
            media_type: "application/octet-stream".to_owned(),
            original_filename: None,
            encryption_mode: FileEncryptionMode::None,
            encryption_metadata: None,
        },
    )
    .await?;
    repo.move_node(
        space_id,
        &MoveNode {
            node_id: staging.id,
            new_parent_node_id: long_parent_id,
            new_name: None,
            expected_parent_id: Some(root_id),
        },
        account,
    )
    .await?;

    assert_path_too_long(
        repo.attach_object_upload(upload_id, space_id, account, None)
            .await,
    );
    let attached: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_objects WHERE object_key = $1")
            .bind(format!("objects/{upload_id}"))
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(attached, 0);
    let upload_state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(upload_state, "uploading");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn mutations_on_soft_deleted_space_return_not_found() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "softdel").await?;
    let repo = FilesRepo::new(db.pool.clone());

    // Seed a folder + text while the space is live.
    let folder = repo
        .insert_folder(
            ws,
            &CreateFolder {
                parent_node_id: root,
                name: "folder".to_owned(),
            },
            account,
        )
        .await?;
    let (doc_node, _) = repo
        .insert_text(ws, root, "doc.md", &content(), account)
        .await?;

    // Soft-delete the space through the production path.
    SpaceRepo::new(db.pool.clone())
        .delete_space(ws, account, account)
        .await?;

    // Every file mutation must now see the space as gone (not_found via lock_space).
    assert_not_found(
        repo.insert_folder(
            ws,
            &CreateFolder {
                parent_node_id: root,
                name: "new-folder".to_owned(),
            },
            account,
        )
        .await,
    );
    assert_not_found(
        repo.insert_text(ws, root, "new-doc.md", &content(), account)
            .await,
    );
    assert_not_found(
        repo.save_text_content(
            ws,
            doc_node.id,
            &content(),
            None,
            account,
            TextMutationKind::Write,
        )
        .await,
    );
    assert_not_found(
        repo.move_node(
            ws,
            &MoveNode {
                node_id: folder.id,
                new_parent_node_id: root,
                new_name: Some("renamed".to_owned()),
                expected_parent_id: None,
            },
            account,
        )
        .await,
    );
    assert_not_found(
        repo.update_node(
            ws,
            &UpdateNode {
                node_id: folder.id,
                name: Some("renamed".to_owned()),
                sort_order: None,
            },
            account,
        )
        .await,
    );
    assert_not_found(repo.soft_delete_node(ws, folder.id, account, false).await);

    db.cleanup().await;
    Ok(())
}

/// `paged_nodes` must order and keyset-paginate correctly for both supported
/// sorts: `updated_at_desc` and `name_asc`. Regression coverage for the
/// query-construction refactor in `files/queries.rs`.
#[tokio::test]
async fn paged_nodes_orders_and_paginates_by_updated_at_desc_and_name_asc()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, ws, root) = space_with_root(&db.pool, "pagednodes").await?;
    let repo = FilesRepo::new(db.pool.clone());

    // Names are intentionally out of alphabetical order so name_asc pagination
    // is not accidentally correct via insertion order.
    let mut nodes = Vec::new();
    for name in ["charlie", "alpha", "bravo"] {
        let node = repo
            .insert_folder(
                ws,
                &CreateFolder {
                    parent_node_id: root,
                    name: name.to_owned(),
                },
                account,
            )
            .await?;
        nodes.push(node);
    }

    // Pin explicit, strictly increasing `updated_at` values so updated_at_desc
    // ordering is unambiguous regardless of wall-clock insert timing.
    let base = Utc::now();
    for (index, node) in nodes.iter().enumerate() {
        let updated_at = base - Duration::seconds((nodes.len() - index) as i64);
        sqlx::query("UPDATE nodes SET updated_at = $2 WHERE id = $1")
            .bind(node.id)
            .bind(updated_at)
            .execute(&db.pool)
            .await?;
    }
    // Insertion order was [charlie, alpha, bravo], so updated_at_desc order
    // (newest first) is [bravo, alpha, charlie].
    let (charlie, alpha, bravo) = (&nodes[0], &nodes[1], &nodes[2]);

    // -- updated_at_desc: first page, then cursor into the rest.
    let (page1, has_more) = repo
        .paged_nodes(ws, None, NodeListSort::UpdatedAtDesc, 2, None)
        .await?;
    assert!(has_more);
    assert_eq!(
        page1.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![bravo.id, alpha.id]
    );

    let last = page1.last().expect("page1 has two entries");
    let cursor = NodeListCursor::UpdatedAtDesc {
        kind: None,
        updated_at: last.updated_at,
        id: last.id,
    };
    let (page2, has_more) = repo
        .paged_nodes(ws, None, NodeListSort::UpdatedAtDesc, 2, Some(&cursor))
        .await?;
    assert!(!has_more);
    assert_eq!(
        page2.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![charlie.id]
    );

    // -- name_asc: first page, then cursor into the rest.
    let (page1, has_more) = repo
        .paged_nodes(ws, None, NodeListSort::NameAsc, 2, None)
        .await?;
    assert!(has_more);
    assert_eq!(
        page1.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![alpha.id, bravo.id]
    );

    let last = page1.last().expect("page1 has two entries");
    let cursor = NodeListCursor::NameAsc {
        kind: None,
        name: last.name.clone(),
        id: last.id,
    };
    let (page2, has_more) = repo
        .paged_nodes(ws, None, NodeListSort::NameAsc, 2, Some(&cursor))
        .await?;
    assert!(!has_more);
    assert_eq!(
        page2.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![charlie.id]
    );

    db.cleanup().await;
    Ok(())
}
