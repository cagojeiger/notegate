//! P0-2: file-tree mutations on a soft-deleted space must read as not_found.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test files_commands`

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
use notegate_db::{FilesRepo, SpaceRepo, TextMutationKind};
use notegate_model::files::{
    CreateFolder, MoveNode, NodeListCursor, NodeListSort, StoredContent, WriteTextBody,
};

fn assert_not_found<T: std::fmt::Debug>(result: Result<T, Error>) {
    match result {
        Err(Error::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
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
        repo.update_node_metadata(ws, folder.id, Some("renamed"), None, account)
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
