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

use common::{TestDb, insert_user_account};
use notegate_core::Error;
use notegate_db::{FilesRepo, SpaceRepo, TextMutationKind};
use notegate_model::files::{CreateFolder, MoveNode, StoredContent, WriteTextBody};
use sqlx::PgPool;
use uuid::Uuid;

fn assert_not_found<T: std::fmt::Debug>(result: Result<T, Error>) {
    match result {
        Err(Error::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

async fn space_with_root(
    pool: &PgPool,
    sub: &str,
) -> Result<(Uuid, Uuid, Uuid), Box<dyn std::error::Error>> {
    let account = insert_user_account(pool, sub, &format!("{sub}@example.com")).await?;
    let space: Uuid =
        sqlx::query_scalar("INSERT INTO spaces (owner_user_id, name) VALUES ($1, $2) RETURNING id")
            .bind(account)
            .bind(format!("ws-{sub}"))
            .fetch_one(pool)
            .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE space_id = $1 AND parent_id IS NULL")
            .bind(space)
            .fetch_one(pool)
            .await?;
    Ok((account, space, root))
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
