//! Object uploads attach file metadata, quota, and history atomically.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account, space_with_root};
use notegate_db::{FilesRepo, ObjectStorageRepo, PurgeRepo, SpaceRepo};
use notegate_model::FileEncryptionMode;
use notegate_model::files::{BeginObjectUpload, CreateFolder};
use uuid::Uuid;

#[tokio::test]
async fn file_schema_is_object_only() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };

    let inline_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name = 'file_inline_contents')",
    )
    .fetch_one(&db.pool)
    .await?;
    let legacy_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'file_objects' \
           AND column_name IN ('storage_kind', 'content_sha256')",
    )
    .fetch_one(&db.pool)
    .await?;
    let object_key_nullable: String = sqlx::query_scalar(
        "SELECT is_nullable FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'file_objects' \
           AND column_name = 'object_key'",
    )
    .fetch_one(&db.pool)
    .await?;
    let detected_media_type_nullable: String = sqlx::query_scalar(
        "SELECT is_nullable FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'file_objects' \
           AND column_name = 'detected_media_type'",
    )
    .fetch_one(&db.pool)
    .await?;

    assert!(!inline_table_exists);
    assert_eq!(legacy_columns, 0);
    assert_eq!(object_key_nullable, "NO");
    assert_eq!(detected_media_type_nullable, "YES");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn object_upload_attach_is_idempotent_and_updates_usage_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "object-attach").await?;
    let other_account_id =
        insert_user_account(&db.pool, "object-other", "object-other@example.com").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    let byte_len = 2 * 1024 * 1024;
    let command = BeginObjectUpload {
        parent_node_id: root_id,
        name: "archive.bin".to_owned(),
        byte_len,
        media_type: "application/octet-stream".to_owned(),
        original_filename: Some("archive.bin".to_owned()),
        encryption_mode: FileEncryptionMode::None,
        encryption_metadata: None,
    };

    let pending = repo
        .insert_object_upload(upload_id, &object_key, space_id, account_id, &command)
        .await?;
    assert_eq!(pending.object_key, object_key);
    assert!(
        repo.object_upload(upload_id, space_id, other_account_id)
            .await?
            .is_none()
    );

    let (node, file) = repo
        .attach_object_upload(upload_id, space_id, account_id, None)
        .await?;
    assert_eq!(file.object_key, object_key);
    assert_eq!(file.byte_len, byte_len);

    let (retried_node, retried_file) = repo
        .attach_object_upload(upload_id, space_id, account_id, None)
        .await?;
    assert_eq!(retried_node.id, node.id);
    assert_eq!(retried_file, file);

    let usage: (i64, i64) = sqlx::query_as(
        "SELECT live_node_count, live_file_bytes FROM space_usage WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(usage, (2, byte_len));

    let create_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_change_events \
         WHERE space_id = $1 AND node_id = $2 AND op_type = 'file.create'",
    )
    .bind(space_id)
    .bind(node.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(create_events, 1);

    let ledger: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state, node_id FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(ledger, ("attached".to_owned(), Some(node.id)));
    assert!(
        !ObjectStorageRepo::new(db.pool.clone())
            .begin_expiry(upload_id)
            .await?
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn stale_uploads_are_claimed_once_for_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "object-cleanup").await?;
    let files = FilesRepo::new(db.pool.clone());
    let cleanup = ObjectStorageRepo::new(db.pool.clone());
    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    files
        .insert_object_upload(
            upload_id,
            &object_key,
            space_id,
            account_id,
            &BeginObjectUpload {
                parent_node_id: root_id,
                name: "stale.bin".to_owned(),
                byte_len: 10,
                media_type: "application/octet-stream".to_owned(),
                original_filename: None,
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    sqlx::query(
        "UPDATE object_storage_objects SET last_activity_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&db.pool)
    .await?;

    let claimed = cleanup
        .claim_cleanup(1_800, 300)
        .await?
        .expect("stale upload should be claimed");
    assert_eq!(claimed.object_key, object_key);
    assert!(cleanup.claim_cleanup(1_800, 300).await?.is_none());
    assert!(
        files
            .touch_object_upload(upload_id, space_id, account_id)
            .await?
    );
    assert!(!cleanup.begin_expiry(upload_id).await?);

    let state: (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT state, retry_after FROM object_storage_objects WHERE id = $1")
            .bind(upload_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(state, ("uploading".to_owned(), None));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_history_retention_starts_at_physical_delete()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "object-retention").await?;
    let files = FilesRepo::new(db.pool.clone());
    let cleanup = ObjectStorageRepo::new(db.pool.clone());
    let upload_id = Uuid::new_v4();
    files
        .insert_object_upload(
            upload_id,
            &format!("objects/{upload_id}"),
            space_id,
            account_id,
            &BeginObjectUpload {
                parent_node_id: root_id,
                name: "expired.bin".to_owned(),
                byte_len: 10,
                media_type: "application/octet-stream".to_owned(),
                original_filename: None,
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    sqlx::query(
        "UPDATE object_storage_objects \
         SET last_activity_at = now() - interval '100 days' WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&db.pool)
    .await?;

    assert!(cleanup.claim_cleanup(1_800, 300).await?.is_some());
    assert!(cleanup.begin_expiry(upload_id).await?);
    assert!(
        cleanup
            .mark_cleanup_failed(upload_id, "unavailable", 120)
            .await?
    );
    let retry: (i32, Option<String>, bool) = sqlx::query_as(
        "SELECT retry_count, last_error_code, retry_after > now() \
         FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(retry, (1, Some("unavailable".to_owned()), true));
    assert!(cleanup.mark_expired(upload_id).await?);
    assert_eq!(cleanup.purge_terminal_history(90, 10).await?, 0);

    sqlx::query(
        "UPDATE object_storage_objects \
         SET deleted_at = now() - interval '91 days' WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&db.pool)
    .await?;
    assert_eq!(cleanup.purge_terminal_history(90, 10).await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn subtree_soft_delete_queues_objects_and_purge_preserves_missed_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "object-purge").await?;
    let files = FilesRepo::new(db.pool.clone());
    let folder = files
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "archive".to_owned(),
            },
            account_id,
        )
        .await?;
    let upload_id = Uuid::new_v4();
    let object_key = format!("objects/{upload_id}");
    files
        .insert_object_upload(
            upload_id,
            &object_key,
            space_id,
            account_id,
            &BeginObjectUpload {
                parent_node_id: folder.id,
                name: "purge.bin".to_owned(),
                byte_len: 10,
                media_type: "application/octet-stream".to_owned(),
                original_filename: None,
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    let (node, _) = files
        .attach_object_upload(upload_id, space_id, account_id, None)
        .await?;
    files
        .soft_delete_node(space_id, folder.id, account_id, true)
        .await?;

    let queued: (String, Option<chrono::DateTime<chrono::Utc>>, Option<Uuid>) = sqlx::query_as(
        "SELECT state, delete_requested_at, node_id \
         FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(queued.0, "delete_pending");
    assert!(queued.1.is_some());
    assert_eq!(queued.2, Some(node.id));

    // Simulate a request missed by an older application version. Hard purge is
    // the final guard that must preserve a physical deletion request.
    sqlx::query(
        "UPDATE object_storage_objects \
         SET state = 'attached', delete_requested_at = NULL WHERE id = $1",
    )
    .bind(upload_id)
    .execute(&db.pool)
    .await?;
    sqlx::query("UPDATE nodes SET purge_after = now() - interval '1 second' WHERE id = $1")
        .bind(folder.id)
        .execute(&db.pool)
        .await?;

    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.nodes_deleted, 1);
    assert_eq!(run.object_deletions_queued, 1);
    let ledger: (String, Option<Uuid>, String) = sqlx::query_as(
        "SELECT state, node_id, object_key FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(ledger, ("delete_pending".to_owned(), None, object_key));
    let file_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM file_objects WHERE node_id = $1)")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(!file_exists);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_soft_delete_queues_all_attached_objects() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "object-space-delete").await?;
    let files = FilesRepo::new(db.pool.clone());
    let upload_id = Uuid::new_v4();
    files
        .insert_object_upload(
            upload_id,
            &format!("objects/{upload_id}"),
            space_id,
            account_id,
            &BeginObjectUpload {
                parent_node_id: root_id,
                name: "space.bin".to_owned(),
                byte_len: 10,
                media_type: "application/octet-stream".to_owned(),
                original_filename: None,
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    files
        .attach_object_upload(upload_id, space_id, account_id, None)
        .await?;

    SpaceRepo::new(db.pool.clone())
        .delete_space(space_id, account_id, account_id)
        .await?;

    let queued: (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT state, delete_requested_at FROM object_storage_objects WHERE id = $1",
    )
    .bind(upload_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(queued.0, "delete_pending");
    assert!(queued.1.is_some());

    db.cleanup().await;
    Ok(())
}
