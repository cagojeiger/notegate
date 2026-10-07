//! Quota follows retained content through trash, database purge and S3 completion.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, attach_file, insert_user_account, legacy_space_with_root, space_with_root};
use notegate_core::{Error, limits::Limits};
use notegate_db::{
    AuditEventRepo, FilesRepo, ObjectStorageRepo, PurgeRepo, SpaceRepo, SpaceUsageRepo, UsageRepo,
};
use notegate_model::files::{StoredContent, WriteTextBody};
use notegate_model::trash::TrashEntryVersion;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn text(body: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(body.to_owned()),
        content_sha256: format!("{:064x}", body.len()),
        byte_len: body.len() as i64,
        line_count: 1,
    }
}

async fn stored(db: &TestDb, space: Uuid) -> Result<(i64, i64), sqlx::Error> {
    sqlx::query_as("SELECT text_bytes, file_bytes FROM space_storage_usage WHERE space_id = $1")
        .bind(space)
        .fetch_one(&db.pool)
        .await
}

async fn version(files: &FilesRepo, owner: Uuid, id: Uuid) -> Result<TrashEntryVersion, Error> {
    let entries = files.list_trash(owner, 100, None).await?;
    Ok(entries
        .iter()
        .find(|entry| entry.id == id)
        .expect("trash entry")
        .into())
}

async fn purge_item(
    db: &TestDb,
    files: &FilesRepo,
    owner: Uuid,
    space: Uuid,
    node: Uuid,
) -> TestResult {
    files
        .request_trash_purge(owner, space, Some(node), version(files, owner, node).await?)
        .await?;
    PurgeRepo::new(db.pool.clone()).run_once().await?;
    Ok(())
}

#[tokio::test]
async fn trash_retains_text_quota_restore_does_not_charge_twice_and_purge_releases_it() -> TestResult
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "stored-text").await?;
    let files = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_text_bytes: 4,
            ..Limits::default()
        },
    );
    let (node, _) = files
        .insert_text(space, root, "note.md", &text("full"), owner)
        .await?;
    files.soft_delete_node(space, node.id, owner, false).await?;
    assert_eq!(stored(&db, space).await?, (4, 0));
    assert!(matches!(
        files
            .insert_text(space, root, "new.md", &text("x"), owner)
            .await,
        Err(Error::Conflict(_))
    ));
    files
        .restore_trashed_node(
            owner,
            space,
            node.id,
            version(&files, owner, node.id).await?,
        )
        .await?;
    assert_eq!(stored(&db, space).await?, (4, 0));
    files.soft_delete_node(space, node.id, owner, false).await?;
    purge_item(&db, &files, owner, space, node.id).await?;
    assert_eq!(stored(&db, space).await?, (0, 0));
    files
        .insert_text(space, root, "new.md", &text("full"), owner)
        .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn file_completion_atomically_releases_quota_and_records_one_receipt() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "stored-file").await?;
    let files = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_file_bytes: 7,
            ..Limits::default()
        },
    );
    let (node, _) = attach_file(&files, space, root, "file.bin", 7, owner).await?;
    let object: Uuid =
        sqlx::query_scalar("SELECT id FROM object_storage_objects WHERE node_id = $1")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    files.soft_delete_node(space, node.id, owner, false).await?;
    assert_eq!(stored(&db, space).await?, (0, 7));
    files
        .restore_trashed_node(
            owner,
            space,
            node.id,
            version(&files, owner, node.id).await?,
        )
        .await?;
    assert_eq!(stored(&db, space).await?, (0, 7));
    files.soft_delete_node(space, node.id, owner, false).await?;
    let deletion = version(&files, owner, node.id).await?.deletion_operation_id;
    purge_item(&db, &files, owner, space, node.id).await?;
    assert_eq!(stored(&db, space).await?, (0, 7));
    assert!(matches!(
        attach_file(&files, space, root, "new.bin", 1, owner).await,
        Err(Error::Conflict(_))
    ));
    let objects = ObjectStorageRepo::new(db.pool.clone());
    assert_eq!(objects.claim_cleanup(1800, 30).await?.unwrap().id, object);
    assert!(
        objects
            .mark_cleanup_failed(object, "unavailable", 30)
            .await?
    );
    assert_eq!(stored(&db, space).await?, (0, 7));

    sqlx::raw_sql("CREATE FUNCTION reject_storage_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected receipt failure'; END; $$; \
        CREATE TRIGGER reject_storage_receipt BEFORE INSERT ON audit_events FOR EACH ROW WHEN (NEW.op_type = 'object.delete') EXECUTE FUNCTION reject_storage_receipt()")
        .execute(&db.pool).await?;
    assert!(objects.mark_deleted(object).await.is_err());
    assert_eq!(stored(&db, space).await?, (0, 7));
    let state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(object)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(state, "delete_pending");
    sqlx::query("DROP TRIGGER reject_storage_receipt ON audit_events")
        .execute(&db.pool)
        .await?;
    let (first, second) = tokio::join!(objects.mark_deleted(object), objects.mark_deleted(object));
    assert_ne!(first?, second?, "exactly one completion wins");
    assert!(!objects.mark_deleted(object).await?);
    assert_eq!(stored(&db, space).await?, (0, 0));
    let events = AuditEventRepo::new(db.pool.clone())
        .list_by_owner(owner, 100, None)
        .await?;
    let receipts: Vec<_> = events
        .iter()
        .filter(|event| event.op_type == "object.delete")
        .collect();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].resource_id, Some(object));
    assert_eq!(receipts[0].operation_id, deletion);
    assert_eq!(receipts[0].actor_account_id, None);
    assert_eq!(
        receipts[0].metadata,
        serde_json::json!({"completion_scope": "s3", "space_id": space})
    );
    attach_file(&files, space, root, "new.bin", 7, owner).await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deleted_space_keeps_owner_accounting_until_s3_completion() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "stored-space").await?;
    let other = insert_user_account(&db.pool, "other", "other@example.com").await?;
    let files = FilesRepo::new(db.pool.clone());
    files
        .insert_text(space, root, "note.md", &text("hello"), owner)
        .await?;
    let (node, _) = attach_file(&files, space, root, "file.bin", 9, owner).await?;
    let object: Uuid =
        sqlx::query_scalar("SELECT id FROM object_storage_objects WHERE node_id = $1")
            .bind(node.id)
            .fetch_one(&db.pool)
            .await?;
    SpaceRepo::new(db.pool.clone())
        .delete_space(space, owner, owner)
        .await?;
    assert_eq!(stored(&db, space).await?, (5, 9));
    let usage = UsageRepo::new(db.pool.clone());
    let snapshot = usage.current_user_usage(owner).await?.unwrap();
    assert!(snapshot.spaces.is_empty());
    assert_eq!(snapshot.deleted_space_count, 1);
    assert_eq!(snapshot.deleted_text_bytes, 5);
    assert_eq!(snapshot.deleted_file_bytes, 9);
    assert_eq!(
        usage
            .current_user_usage(other)
            .await?
            .unwrap()
            .deleted_space_count,
        0
    );
    assert!(
        usage
            .current_user_usage(other)
            .await?
            .unwrap()
            .spaces
            .is_empty()
    );
    files
        .request_trash_purge(owner, space, None, version(&files, owner, space).await?)
        .await?;
    let purge = PurgeRepo::new(db.pool.clone());
    for _ in 0..4 {
        purge.run_once().await?;
    }
    assert_eq!(stored(&db, space).await?, (0, 9));
    let links: (Option<Uuid>, Uuid) =
        sqlx::query_as("SELECT space_id, usage_space_id FROM object_storage_objects WHERE id = $1")
            .bind(object)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(links, (None, space));
    let snapshot = usage.current_user_usage(owner).await?.unwrap();
    assert!(snapshot.spaces.is_empty());
    assert_eq!(snapshot.deleted_text_bytes, 0);
    assert_eq!(snapshot.deleted_file_bytes, 9);
    assert_eq!(
        snapshot,
        usage.current_user_usage(owner).await?.unwrap(),
        "stable polling response"
    );
    assert!(
        ObjectStorageRepo::new(db.pool.clone())
            .mark_deleted(object)
            .await?
    );
    assert!(
        usage
            .current_user_usage(owner)
            .await?
            .unwrap()
            .spaces
            .is_empty()
    );
    assert_eq!(
        usage
            .current_user_usage(owner)
            .await?
            .unwrap()
            .deleted_space_count,
        0
    );
    purge.run_once().await?;
    let scopes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM space_storage_usage WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(scopes, 0);
    let events = AuditEventRepo::new(db.pool.clone())
        .list_by_owner(owner, 100, None)
        .await?;
    assert!(
        events
            .iter()
            .any(|event| event.op_type == "object.delete" && event.resource_id == Some(object))
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn reconciliation_counts_trash_and_pending_objects_and_serializes_completion() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "stored-reconcile").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (note, _) = files
        .insert_text(space, root, "note.md", &text("hello"), owner)
        .await?;
    let (file, _) = attach_file(&files, space, root, "file.bin", 8, owner).await?;
    let object: Uuid =
        sqlx::query_scalar("SELECT id FROM object_storage_objects WHERE node_id = $1")
            .bind(file.id)
            .fetch_one(&db.pool)
            .await?;
    files.soft_delete_node(space, note.id, owner, false).await?;
    files.soft_delete_node(space, file.id, owner, false).await?;
    purge_item(&db, &files, owner, space, file.id).await?;
    sqlx::query(
        "UPDATE space_storage_usage SET text_bytes = 0, file_bytes = 0 WHERE space_id = $1",
    )
    .bind(space)
    .execute(&db.pool)
    .await?;
    let reconcile = SpaceUsageRepo::new(db.pool.clone());
    reconcile.reconcile_space(space).await?;
    assert_eq!(stored(&db, space).await?, (5, 8));
    let objects = ObjectStorageRepo::new(db.pool.clone());
    let (repaired, completed) = tokio::join!(
        reconcile.reconcile_space(space),
        objects.mark_deleted(object)
    );
    repaired?;
    assert!(completed?);
    assert_eq!(stored(&db, space).await?, (5, 0));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn migration_backfills_pending_bytes_without_guessing_legacy_orphan_ownership() -> TestResult
{
    let Some(db) = TestDb::setup_before(51).await? else {
        return Ok(());
    };
    let (owner, space, root) = legacy_space_with_root(&db.pool, "stored-migration").await?;
    for deleted in [false, true] {
        let node = Uuid::new_v4();
        sqlx::query("INSERT INTO nodes(id, space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) VALUES ($1, $2, $3, $4, 'text', $5, $5)")
            .bind(node).bind(space).bind(root).bind(node.to_string()).bind(owner).execute(&db.pool).await?;
        sqlx::query("INSERT INTO text_objects(node_id, space_id, content_text, byte_len, line_count, created_by_account_id, updated_by_account_id) VALUES ($1, $2, 'body', 4, 1, $3, $3)")
            .bind(node).bind(space).bind(owner).execute(&db.pool).await?;
        if deleted {
            sqlx::query("UPDATE nodes SET deleted_at = now(), deleted_by_account_id = $2, purge_after = now() + interval '30 days', deletion_target_node_id = id WHERE id = $1")
                .bind(node).bind(owner).execute(&db.pool).await?;
        }
    }
    for state in [
        "delete_pending",
        "uploading",
        "expire_pending",
        "deleted",
        "expired",
    ] {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO object_storage_objects(id, object_key, space_id, name, declared_byte_len, media_type, state) VALUES ($1, $2, $3, 'legacy.bin', 7, 'application/octet-stream', $4)")
            .bind(id).bind(format!("objects/{id}")).bind(space).bind(state).execute(&db.pool).await?;
    }
    let orphan = Uuid::new_v4();
    sqlx::query("INSERT INTO object_storage_objects(id, object_key, name, declared_byte_len, media_type, state) VALUES ($1, $2, 'orphan.bin', 11, 'application/octet-stream', 'delete_pending')")
        .bind(orphan).bind(format!("objects/{orphan}")).execute(&db.pool).await?;
    db.apply_migration(51).await?;
    assert_eq!(stored(&db, space).await?, (8, 7));
    sqlx::query("UPDATE object_storage_objects SET space_id = NULL WHERE space_id = $1")
        .bind(space)
        .execute(&db.pool)
        .await?;
    assert_eq!(stored(&db, space).await?, (8, 7));
    let scopes: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT usage_space_id FROM object_storage_objects WHERE usage_space_id IS NOT NULL")
        .fetch_all(&db.pool).await?;
    assert_eq!(scopes, vec![space]);
    assert!(
        ObjectStorageRepo::new(db.pool.clone())
            .mark_deleted(orphan)
            .await?
    );
    assert_eq!(stored(&db, space).await?, (8, 7));
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT owner_user_id FROM audit_events WHERE resource_id = $1 AND op_type = 'object.delete'")
        .bind(orphan).fetch_one(&db.pool).await?;
    assert_eq!(owner, None);
    db.cleanup().await;
    Ok(())
}
