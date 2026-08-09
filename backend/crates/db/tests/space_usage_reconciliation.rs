//! Integration tests for exact Space usage reconciliation and mutation gates.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, attach_file, space_with_root};
use notegate_core::Error;
use notegate_db::{FilesRepo, SpaceRepo, SpaceUsageRepo, UsageCounts, UsageReconcileResult};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
use uuid::Uuid;

const SPACE_GATE_NAMESPACE: u64 = 0x4e47_5350_4143_4501;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: format!("{:064x}", content.len()),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

fn space_gate_seed(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
}

async fn acquire_gate(
    tx: &mut sqlx::PgConnection,
    space_id: Uuid,
    shared: bool,
) -> Result<(), sqlx::Error> {
    let query = if shared {
        "SELECT pg_advisory_xact_lock_shared(hashtextextended(current_schema(), $1))"
    } else {
        "SELECT pg_advisory_xact_lock(hashtextextended(current_schema(), $1))"
    };
    sqlx::query(sqlx::AssertSqlSafe(query))
        .bind(space_gate_seed(space_id))
        .execute(tx)
        .await?;
    Ok(())
}

#[tokio::test]
async fn reconciliation_repairs_drift() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "reconcile-drift").await?;
    let files = FilesRepo::new(db.pool.clone());
    files
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    attach_file(&files, space_id, root_id, "asset.bin", 3, account).await?;
    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = 99, live_text_bytes = 999, live_file_bytes = 888 \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .reconcile_space(space_id)
            .await?,
        UsageReconcileResult::Reconciled {
            previous: Some(UsageCounts {
                live_node_count: 99,
                live_text_bytes: 999,
                live_file_bytes: 888,
            }),
            actual: UsageCounts {
                live_node_count: 3,
                live_text_bytes: 5,
                live_file_bytes: 3,
            },
        }
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_treats_a_deleted_space_as_complete()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, _) = space_with_root(&db.pool, "reconcile-deleted").await?;
    SpaceRepo::new(db.pool.clone())
        .delete_space(space_id, account, account)
        .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .reconcile_space(space_id)
            .await?,
        UsageReconcileResult::Deleted
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn operator_reconciliation_visits_every_live_space() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, first_space, first_root) =
        space_with_root(&db.pool, "full-reconcile-first").await?;
    let (_, second_space, _) = space_with_root(&db.pool, "full-reconcile-second").await?;
    FilesRepo::new(db.pool.clone())
        .insert_text(first_space, first_root, "note.md", &text("hello"), account)
        .await?;
    sqlx::query(
        "UPDATE space_usage \
         SET live_node_count = 99, live_text_bytes = 999, live_file_bytes = 888",
    )
    .execute(&db.pool)
    .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .reconcile_all_live_spaces()
            .await?,
        2
    );
    let first: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(first_space)
    .fetch_one(&db.pool)
    .await?;
    let second: (i64, i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_text_bytes, live_file_bytes \
         FROM space_usage WHERE space_id = $1",
    )
    .bind(second_space)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(first, (2, 5, 0));
    assert_eq!(second, (1, 0, 0));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn missing_counter_is_recreated() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-missing-counter").await?;
    sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .reconcile_space(space_id)
            .await?,
        UsageReconcileResult::Reconciled {
            previous: None,
            actual: UsageCounts {
                live_node_count: 1,
                live_text_bytes: 0,
                live_file_bytes: 0,
            },
        }
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn busy_space_returns_without_blocking() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "reconcile-busy").await?;
    let mut mutation_tx = db.pool.begin().await?;
    acquire_gate(&mut mutation_tx, space_id, true).await?;

    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .reconcile_space(space_id)
            .await?,
        UsageReconcileResult::Busy
    );

    mutation_tx.commit().await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn exclusive_reconciliation_gate_rejects_then_releases_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "reconcile-mutation").await?;
    let files = FilesRepo::new(db.pool.clone());
    let command = CreateFolder {
        parent_node_id: root_id,
        name: "blocked".to_owned(),
    };
    let mut reconciliation_tx = db.pool.begin().await?;
    acquire_gate(&mut reconciliation_tx, space_id, false).await?;

    let error = files
        .insert_folder(space_id, &command, account)
        .await
        .expect_err("mutation must fail while reconciliation holds the gate");
    assert!(matches!(
        error,
        Error::UsageRecalculationInProgress {
            retry_after_seconds: 5
        }
    ));

    reconciliation_tx.commit().await?;
    files.insert_folder(space_id, &command, account).await?;

    db.cleanup().await;
    Ok(())
}
