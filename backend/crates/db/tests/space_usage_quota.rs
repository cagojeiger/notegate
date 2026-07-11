//! Integration tests for authoritative Space quota enforcement.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, space_with_root};
use notegate_core::Error;
use notegate_core::limits::Limits;
use notegate_db::{FilesRepo, FullUsageReconcileRun, SpaceUsageRepo, TextMutationKind};
use notegate_model::FileEncryptionMode;
use notegate_model::files::{CopyNode, CreateFolder, StoredContent, StoredFile, WriteTextBody};
use sqlx::PgPool;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

fn file(bytes: &[u8]) -> StoredFile {
    StoredFile {
        bytes: bytes.to_vec(),
        content_sha256: "f".repeat(64),
        byte_len: bytes.len() as i64,
        media_type: "application/octet-stream".to_owned(),
        original_filename: Some("asset.bin".to_owned()),
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    }
}

async fn assert_usage(pool: &PgPool, space_id: Uuid, expected: (i64, i64)) -> TestResult {
    let stored: (i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_content_bytes FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(pool)
    .await?;
    let exact = SpaceUsageRepo::new(pool.clone())
        .calculate_exact_usage(space_id)
        .await?;
    assert_eq!(stored, expected);
    assert_eq!(stored, (exact.live_node_count, exact.live_content_bytes));
    Ok(())
}

#[tokio::test]
async fn counter_enforces_node_and_content_limits() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-limits").await?;
    let repo = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 3,
            space_max_content_bytes: 5,
            folder_max_children: 10,
        },
    );
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
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    assert_usage(&db.pool, space_id, (3, 5)).await?;

    let node_error = repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "overflow".to_owned(),
            },
            account,
        )
        .await
        .expect_err("node quota must be enforced from the counter");
    assert!(matches!(
        node_error,
        Error::Conflict(message) if message.contains("maximum of 3 live nodes")
    ));

    let content_error = repo
        .save_text_content(
            space_id,
            text_node.id,
            &text("hello!"),
            None,
            account,
            TextMutationKind::Write,
        )
        .await
        .expect_err("content quota must roll back the save");
    assert!(matches!(
        content_error,
        Error::Conflict(message) if message.contains("maximum of 5 bytes")
    ));
    assert_usage(&db.pool, space_id, (3, 5)).await?;

    repo.soft_delete_node(space_id, folder.id, account, false)
        .await?;
    let file_error = repo
        .insert_file(space_id, root_id, "blocked.bin", &file(b"x"), account)
        .await
        .expect_err("content quota must roll back the file create");
    assert!(matches!(file_error, Error::Conflict(_)));
    assert_usage(&db.pool, space_id, (2, 5)).await?;

    repo.save_text_content(
        space_id,
        text_node.id,
        &text("hey"),
        None,
        account,
        TextMutationKind::Write,
    )
    .await?;
    repo.insert_file(space_id, root_id, "asset.bin", &file(b"ab"), account)
        .await?;
    assert_usage(&db.pool, space_id, (3, 5)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn subtree_copy_checks_its_full_delta() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-copy-limit").await?;
    let repo = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 10,
            space_max_content_bytes: 9,
            folder_max_children: 10,
        },
    );
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
    repo.insert_text(space_id, folder.id, "note.md", &text("hello"), account)
        .await?;

    let error = repo
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
        .await
        .expect_err("copy must reserve the complete subtree usage before inserting");
    assert!(matches!(
        error,
        Error::Conflict(message) if message.contains("maximum of 9 bytes")
    ));
    assert_usage(&db.pool, space_id, (3, 5)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_node_creates_respect_the_boundary() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-node-race").await?;
    let repo = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 2,
            space_max_content_bytes: 100,
            folder_max_children: 10,
        },
    );
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
    let mut succeeded = 0;
    let mut rejected = 0;
    for result in [first_result, second_result] {
        match result {
            Ok(_) => succeeded += 1,
            Err(Error::Conflict(_)) => rejected += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!((succeeded, rejected), (1, 1));
    assert_usage(&db.pool, space_id, (2, 0)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_content_creates_respect_the_boundary() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) =
        space_with_root(&db.pool, "space-usage-content-race").await?;
    let repo = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 10,
            space_max_content_bytes: 5,
            folder_max_children: 10,
        },
    );
    let first_repo = repo.clone();
    let second_repo = repo.clone();
    let first_file = file(b"abc");
    let second_file = file(b"def");

    let (first_result, second_result) = tokio::join!(
        first_repo.insert_file(space_id, root_id, "first.bin", &first_file, account),
        second_repo.insert_file(space_id, root_id, "second.bin", &second_file, account),
    );
    let mut succeeded = 0;
    let mut rejected = 0;
    for result in [first_result, second_result] {
        match result {
            Ok(_) => succeeded += 1,
            Err(Error::Conflict(_)) => rejected += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!((succeeded, rejected), (1, 1));
    assert_usage(&db.pool, space_id, (2, 3)).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn quota_reducing_mutations_remain_available_above_the_limit() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-reduce").await?;
    let setup_repo = FilesRepo::new(db.pool.clone());
    let folder = setup_repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "docs".to_owned(),
            },
            account,
        )
        .await?;
    let (text_node, _) = setup_repo
        .insert_text(space_id, root_id, "note.md", &text("hello"), account)
        .await?;
    let restricted_repo = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 1,
            space_max_content_bytes: 1,
            folder_max_children: 10,
        },
    );

    restricted_repo
        .soft_delete_node(space_id, folder.id, account, false)
        .await?;
    restricted_repo
        .save_text_content(
            space_id,
            text_node.id,
            &text("x"),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    assert_usage(&db.pool, space_id, (2, 1)).await?;

    let error = restricted_repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "still-over".to_owned(),
            },
            account,
        )
        .await
        .expect_err("quota-increasing mutations remain blocked");
    assert!(matches!(error, Error::Conflict(_)));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn missing_counter_rolls_back_mutation_and_full_recalculation_repairs_it() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "space-usage-missing").await?;
    let repo = FilesRepo::new(db.pool.clone());
    sqlx::query("DELETE FROM space_usage WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let error = repo
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "blocked".to_owned(),
            },
            account,
        )
        .await
        .expect_err("a missing authoritative counter must reject the mutation");
    assert!(matches!(
        error,
        Error::Internal(message) if message == "live space is missing its usage counter"
    ));
    assert_eq!(
        SpaceUsageRepo::new(db.pool.clone())
            .calculate_exact_usage(space_id)
            .await?
            .live_node_count,
        1
    );

    let usage_repo = SpaceUsageRepo::new(db.pool.clone());
    let mut recalculation = FullUsageReconcileRun::MutationsActive;
    for _ in 0..20 {
        recalculation = usage_repo.run_full_recalculation().await?;
        if recalculation != FullUsageReconcileRun::MutationsActive {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        recalculation,
        FullUsageReconcileRun::Recalculated {
            spaces_recalculated: 1
        }
    );
    repo.insert_folder(
        space_id,
        &CreateFolder {
            parent_node_id: root_id,
            name: "recovered".to_owned(),
        },
        account,
    )
    .await?;
    assert_usage(&db.pool, space_id, (2, 0)).await?;

    db.cleanup().await;
    Ok(())
}
