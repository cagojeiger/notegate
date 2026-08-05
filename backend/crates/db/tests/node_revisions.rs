mod common;

use common::{TestDb, space_with_root};
use notegate_core::Error;
use notegate_db::{FilesRepo, TextMutationGuard, TextMutationKind};
use notegate_model::files::{CreateFolder, StoredContent, UpdateNode, WriteTextBody};

fn text(content: &str) -> StoredContent {
    let checksum = content.bytes().fold(0_u64, |value, byte| {
        value.wrapping_mul(31) + u64::from(byte)
    });
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: format!("{checksum:064x}"),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

#[tokio::test]
async fn revision_changes_once_and_rejects_stale_no_ops() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "node-revisions").await?;
    let repo = FilesRepo::new(db.pool.clone());

    let folder = repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "drafts".to_owned(),
            },
            account,
        )
        .await?;
    assert_eq!(folder.revision, 1);

    let renamed = repo
        .update_node(
            space_id,
            &UpdateNode {
                node_id: folder.id,
                name: Some("published".to_owned()),
                sort_order: None,
                expected_revision: folder.revision,
            },
            account,
        )
        .await?;
    assert_eq!(renamed.revision, 2);

    let no_op = repo
        .update_node(
            space_id,
            &UpdateNode {
                node_id: folder.id,
                name: Some("published".to_owned()),
                sort_order: None,
                expected_revision: renamed.revision,
            },
            account,
        )
        .await?;
    assert_eq!(no_op.revision, renamed.revision);

    let stale = repo
        .update_node(
            space_id,
            &UpdateNode {
                node_id: folder.id,
                name: Some("published".to_owned()),
                sort_order: None,
                expected_revision: folder.revision,
            },
            account,
        )
        .await;
    assert!(matches!(stale, Err(Error::Conflict(_))));

    let (node, _) = repo
        .insert_text(space_id, root_id, "note.md", &text("v1"), account)
        .await?;
    let (written, _) = repo
        .save_text_content(
            space_id,
            node.id,
            &text("v2"),
            TextMutationGuard {
                revision: node.revision,
                sha256: None,
            },
            account,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(written.revision, 2);

    let (same, _) = repo
        .save_text_content(
            space_id,
            node.id,
            &text("v2"),
            TextMutationGuard {
                revision: written.revision,
                sha256: None,
            },
            account,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(same.revision, written.revision);

    let stale = repo
        .save_text_content(
            space_id,
            node.id,
            &text("v2"),
            TextMutationGuard {
                revision: node.revision,
                sha256: None,
            },
            account,
            TextMutationKind::Write,
        )
        .await;
    assert!(matches!(stale, Err(Error::Conflict(_))));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_updates_with_one_revision_have_one_winner()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "node-revision-race").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let folder = repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "source".to_owned(),
            },
            account,
        )
        .await?;

    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first = UpdateNode {
        node_id: folder.id,
        name: Some("first".to_owned()),
        sort_order: None,
        expected_revision: folder.revision,
    };
    let second = UpdateNode {
        node_id: folder.id,
        name: Some("second".to_owned()),
        sort_order: None,
        expected_revision: folder.revision,
    };

    let (first_result, second_result) = tokio::join!(
        first_repo.update_node(space_id, &first, account),
        second_repo.update_node(space_id, &second, account),
    );
    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Conflict(_))))
            .count(),
        1
    );
    let current = repo.find_node(space_id, folder.id).await?;
    assert_eq!(current.as_ref().map(|node| node.revision), Some(2));

    db.cleanup().await;
    Ok(())
}
