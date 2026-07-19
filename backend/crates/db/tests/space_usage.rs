//! Integration tests for transactional Space usage counters.

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
use notegate_db::{FilesRepo, SpaceUsageRepo, TextMutationKind, UsageReconcileExecution};
use notegate_model::files::{CopyNode, CreateFolder, MoveNode, StoredContent, WriteTextBody};
use sqlx::PgPool;
use uuid::Uuid;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

async fn assert_usage(
    pool: &PgPool,
    space_id: Uuid,
    expected: (i64, i64, i64),
) -> Result<(), Box<dyn std::error::Error>> {
    let stored: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(pool)
    .await?;
    let exact = SpaceUsageRepo::new(pool.clone())
        .calculate_exact_usage(space_id)
        .await?;

    assert_eq!(stored, expected);
    assert_eq!(
        stored,
        (
            exact.live_node_count,
            exact.live_text_bytes,
            exact.live_file_bytes,
        )
    );
    Ok(())
}

#[tokio::test]
async fn usage_counter_tracks_file_tree_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage").await?;
    let repo = FilesRepo::new(db.pool.clone());

    assert_usage(&db.pool, space_id, (1, 0, 0)).await?;

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
    let (text_node, _) = repo
        .insert_text(space_id, folder.id, "note.md", &text("hello"), account)
        .await?;
    attach_file(&repo, space_id, root_id, "asset.bin", 3, account).await?;
    assert_usage(&db.pool, space_id, (4, 5, 3)).await?;

    repo.save_text_content(
        space_id,
        text_node.id,
        &text("hello world"),
        None,
        account,
        TextMutationKind::Write,
    )
    .await?;
    repo.save_text_content(
        space_id,
        text_node.id,
        &text("hello world"),
        None,
        account,
        TextMutationKind::Write,
    )
    .await?;
    assert_usage(&db.pool, space_id, (4, 11, 3)).await?;

    let (copied, counts) = repo
        .copy_node(
            space_id,
            &CopyNode {
                node_id: folder.id,
                new_parent_node_id: root_id,
                new_name: "docs-copy".to_owned(),
                recursive: true,
            },
            account,
        )
        .await?;
    assert_eq!(counts.nodes, 2);
    assert_usage(&db.pool, space_id, (6, 22, 3)).await?;

    repo.move_node(
        space_id,
        &MoveNode {
            node_id: copied.id,
            new_parent_node_id: root_id,
            new_name: Some("docs-archive".to_owned()),
            expected_parent_id: None,
        },
        account,
    )
    .await?;
    repo.update_node_metadata(space_id, copied.id, None, Some(2_000), account)
        .await?;
    assert_usage(&db.pool, space_id, (6, 22, 3)).await?;

    repo.soft_delete_node(space_id, folder.id, account, true)
        .await?;
    assert_usage(&db.pool, space_id, (4, 11, 3)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn usage_drift_rolls_back_mutation_until_reconciled() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-drift").await?;
    let repo = FilesRepo::new(db.pool.clone());
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

    sqlx::query("UPDATE space_usage SET live_node_count = 1 WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let error = repo
        .soft_delete_node(space_id, folder.id, account, false)
        .await
        .expect_err("counter underflow must reject the source mutation");
    assert!(matches!(
        error,
        Error::Internal(message) if message == "space usage counter underflow"
    ));
    let folder_is_live: bool =
        sqlx::query_scalar("SELECT deleted_at IS NULL FROM nodes WHERE space_id = $1 AND id = $2")
            .bind(space_id)
            .bind(folder.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(folder_is_live);

    sqlx::query("INSERT INTO space_usage_reconcile_jobs (space_id) VALUES ($1)")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        SpaceUsageRepo::new(db.pool.clone())
            .execute_next_reconciliation()
            .await?,
        UsageReconcileExecution::Succeeded { space_id: id, .. } if id == space_id
    ));
    repo.soft_delete_node(space_id, folder.id, account, false)
        .await?;
    assert_usage(&db.pool, space_id, (1, 0, 0)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn migration_backfills_existing_space_usage() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-backfill").await?;
    let repo = FilesRepo::new(db.pool.clone());
    repo.insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    attach_file(&repo, space_id, root_id, "asset.bin", 3, account).await?;

    sqlx::raw_sql(
        "DROP TRIGGER spaces_create_usage ON spaces; \
         DROP FUNCTION create_space_usage(); \
         DROP TABLE space_usage_reconcile_executions; \
         DROP TABLE space_usage_reconcile_jobs; \
         DROP TABLE space_usage; \
         DROP INDEX agents_owner_user_idx;",
    )
    .execute(&db.pool)
    .await?;
    sqlx::raw_sql(include_str!("../migrations/0012_space_usage.sql"))
        .execute(&db.pool)
        .await?;
    sqlx::raw_sql(include_str!(
        "../migrations/0013_split_space_usage_bytes.sql"
    ))
    .execute(&db.pool)
    .await?;

    assert_usage(&db.pool, space_id, (3, 5, 3)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_mutations_do_not_lose_usage_deltas() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-concurrent").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first = CreateFolder {
        parent_node_id: root_id,
        name: "first".to_owned(),
    };
    let second = CreateFolder {
        parent_node_id: root_id,
        name: "second".to_owned(),
    };

    let (first_result, second_result) = tokio::join!(
        first_repo.insert_folder(space_id, &first, account),
        second_repo.insert_folder(space_id, &second, account),
    );
    first_result?;
    second_result?;
    assert_usage(&db.pool, space_id, (3, 0, 0)).await?;

    db.cleanup().await;
    Ok(())
}
