//! Integration tests for soft-delete hard purge.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_db::PurgeRepo;
use sqlx::Row as _;
use uuid::Uuid;

const PURGE_ADVISORY_LOCK_KEY: i64 = 0x4e47_5055_5247_4501;

#[tokio::test]
async fn purge_deletes_due_workspaces_and_nodes() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "purger", "purger@example.test").await?;

    let due_workspace: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name, deleted_at, deleted_by, purge_after) \
         VALUES ($1, 'due-workspace', now() - interval '40 days', $1, now() - interval '1 day') \
         RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;

    let live_workspace: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name) VALUES ($1, 'live-workspace') RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE workspace_id = $1 AND parent_id IS NULL")
            .bind(live_workspace)
            .fetch_one(&db.pool)
            .await?;
    let due_node: Uuid = sqlx::query_scalar(
        "INSERT INTO nodes \
         (workspace_id, parent_id, name, kind, created_by, updated_by, deleted_by, deleted_at, purge_after) \
         VALUES ($1, $2, 'old.md', 'document', $3, $3, $3, now() - interval '40 days', now() - interval '1 day') \
         RETURNING id",
    )
    .bind(live_workspace)
    .bind(root)
    .bind(user)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO documents (node_id, workspace_id, content_md, created_by, updated_by) \
         VALUES ($1, $2, 'old', $3, $3)",
    )
    .bind(due_node)
    .bind(live_workspace)
    .bind(user)
    .execute(&db.pool)
    .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert!(run.lock_acquired);
    assert_eq!(run.workspaces_deleted, 1);
    assert_eq!(run.nodes_deleted, 1);

    let workspace_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE id = $1")
            .bind(due_workspace)
            .fetch_optional(&db.pool)
            .await?;
    assert!(workspace_exists.is_none());

    let node_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM nodes WHERE id = $1")
        .bind(due_node)
        .fetch_optional(&db.pool)
        .await?;
    assert!(node_exists.is_none());

    let document_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT node_id FROM documents WHERE node_id = $1")
            .bind(due_node)
            .fetch_optional(&db.pool)
            .await?;
    assert!(document_exists.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_skips_when_advisory_lock_is_held() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "locked", "locked@example.test").await?;
    let due_workspace: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name, deleted_at, deleted_by, purge_after) \
         VALUES ($1, 'locked-workspace', now() - interval '40 days', $1, now() - interval '1 day') \
         RETURNING id",
    )
    .bind(user)
    .fetch_one(&db.pool)
    .await?;

    let mut tx = db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(PURGE_ADVISORY_LOCK_KEY)
        .execute(&mut *tx)
        .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert!(!run.lock_acquired);
    assert_eq!(run.workspaces_deleted, 0);
    assert_eq!(run.nodes_deleted, 0);

    let still_exists = sqlx::query("SELECT id FROM workspaces WHERE id = $1")
        .bind(due_workspace)
        .fetch_one(&db.pool)
        .await?
        .get::<Uuid, _>("id");
    assert_eq!(still_exists, due_workspace);

    tx.commit().await?;
    db.cleanup().await;
    Ok(())
}
