#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{DateTime, Duration, Utc};
use common::{TestDb, attach_file, insert_user_account, legacy_space_with_root, space_with_root};
use notegate_core::{Error, limits::Limits};
use notegate_db::{AgentRepo, AuditEventRepo, ConnectionRepo, FilesRepo, PurgeRepo, SpaceRepo};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
use notegate_model::trash::TrashEntryVersion;
use notegate_model::{ConnectAgent, CreateAgent, Permission};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn folder(
    repo: &FilesRepo,
    owner: Uuid,
    space: Uuid,
    parent: Uuid,
    name: &str,
) -> Result<notegate_model::Node, Error> {
    repo.insert_folder(
        space,
        &CreateFolder {
            parent_node_id: parent,
            name: name.to_owned(),
        },
        owner,
    )
    .await
}

async fn entry_version(
    pool: &sqlx::PgPool,
    space: Uuid,
    node: Option<Uuid>,
) -> Result<TrashEntryVersion, Box<dyn std::error::Error>> {
    let (deleted_at, deletion_operation_id): (DateTime<Utc>, Option<Uuid>) = match node {
        Some(node) => sqlx::query_as("SELECT COALESCE(deleted_at, created_at), deletion_operation_id FROM nodes WHERE space_id = $1 AND id = $2")
            .bind(space).bind(node).fetch_one(pool).await?,
        None => sqlx::query_as("SELECT COALESCE(deleted_at, created_at), deletion_operation_id FROM spaces WHERE id = $1")
            .bind(space).fetch_one(pool).await?,
    };
    Ok(TrashEntryVersion {
        deleted_at,
        deletion_operation_id,
    })
}

#[tokio::test]
async fn trash_migrations_preserve_legacy_targets_and_nullable_event_links() -> TestResult {
    let Some(db) = TestDb::setup_before(45).await? else {
        return Ok(());
    };
    let (owner, space, root) =
        legacy_space_with_root(&db.pool, "trash-operation-migration").await?;
    let item = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, space_id, parent_id, name, kind, created_by_account_id, \
             updated_by_account_id, deleted_by_account_id, deleted_at, purge_after, deletion_root_id) \
         VALUES ($1, $2, $3, 'notes', 'folder', $4, $4, $4, now(), now() + interval '30 days', $1)",
    ).bind(item).bind(space).bind(root).bind(owner).execute(&db.pool).await?;
    sqlx::query("INSERT INTO file_change_events (space_id, node_id, actor_account_id, op_type) VALUES ($1, $2, $3, 'item.delete')")
        .bind(space).bind(item).bind(owner).execute(&db.pool).await?;
    let child = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, space_id, parent_id, name, kind, created_by_account_id, \
             updated_by_account_id, deleted_by_account_id, deleted_at, purge_after, deletion_root_id) \
         SELECT $1, space_id, id, 'child', 'folder', $2, $2, $2, deleted_at, purge_after, id \
         FROM nodes WHERE id = $3",
    )
    .bind(child)
    .bind(owner)
    .bind(item)
    .execute(&db.pool)
    .await?;
    let before: Vec<(Uuid, Uuid, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, deletion_root_id, deleted_at, purge_after FROM nodes \
         WHERE space_id = $1 AND deleted_at IS NOT NULL ORDER BY id",
    )
    .bind(space)
    .fetch_all(&db.pool)
    .await?;
    db.apply_migration(45).await?;
    db.apply_migration(46).await?;
    let after: Vec<(Uuid, Uuid, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, deletion_target_node_id, deleted_at, purge_after FROM nodes \
         WHERE space_id = $1 AND deleted_at IS NOT NULL ORDER BY id",
    )
    .bind(space)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(after, before);
    let repo = FilesRepo::new(db.pool.clone());
    let list = repo.list_trash(owner, 100, None).await?;
    assert!(list[0].recoverable);
    assert_eq!(list[0].deletion_operation_id, None);
    let stale = TrashEntryVersion {
        deleted_at: list[0].deleted_at - Duration::microseconds(1),
        deletion_operation_id: None,
    };
    assert!(matches!(
        repo.restore_trashed_node(owner, space, item, stale).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        repo.request_trash_purge(owner, space, Some(item), stale)
            .await,
        Err(Error::Conflict(_))
    ));
    let events = repo
        .list_file_change_events(space, Some(item), 100, None)
        .await?;
    assert_eq!(events[0].operation_id, None);
    repo.restore_trashed_node(
        owner,
        space,
        item,
        entry_version(&db.pool, space, Some(item)).await?,
    )
    .await?;
    assert!(repo.find_node(space, child).await?.is_some());
    let events = repo
        .list_file_change_events(space, Some(item), 100, None)
        .await?;
    assert_eq!(events[0].op_type, "item.restore");
    assert!(events[0].operation_id.is_some());
    assert!(events[0].metadata["related_deletion_operation_id"].is_null());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn folder_restore_keeps_original_text_and_file_but_not_previously_deleted_children()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-folder").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let parent = folder(&repo, owner, space, root, "notes").await?;
    let old = folder(&repo, owner, space, parent.id, "old").await?;
    repo.soft_delete_node(space, old.id, owner, true).await?;
    let content = StoredContent {
        body: WriteTextBody::Plain("kept".to_owned()),
        content_sha256: "a".repeat(64),
        byte_len: 4,
        line_count: 1,
    };
    let (text, _) = repo
        .insert_text(space, parent.id, "note.md", &content, owner)
        .await?;
    let (file, _) = attach_file(&repo, space, parent.id, "data.bin", 7, owner).await?;
    repo.soft_delete_node(space, parent.id, owner, true).await?;
    assert!(repo.find_node(space, text.id).await?.is_none());
    assert!(matches!(
        repo.request_trash_purge(
            owner,
            space,
            Some(file.id),
            entry_version(&db.pool, space, Some(file.id)).await?
        )
        .await,
        Err(Error::Conflict(_))
    ));
    let trash = repo.list_trash(owner, 100, None).await?;
    assert_eq!(trash.len(), 2);
    let deletion = trash
        .iter()
        .find(|i| i.id == parent.id)
        .unwrap()
        .deletion_operation_id
        .unwrap();
    let old_deletion = trash
        .iter()
        .find(|i| i.id == old.id)
        .unwrap()
        .deletion_operation_id
        .unwrap();
    assert_ne!(deletion, old_deletion);
    let grouped: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM nodes WHERE space_id = $1 AND deletion_operation_id = $2 ORDER BY id",
    )
    .bind(space)
    .bind(deletion)
    .fetch_all(&db.pool)
    .await?;
    let mut expected = vec![parent.id, text.id, file.id];
    expected.sort();
    assert_eq!(grouped, expected);
    assert!(
        trash
            .iter()
            .any(|i| i.id == parent.id && i.path == "/notes" && i.recoverable)
    );
    assert!(!trash.iter().find(|i| i.id == old.id).unwrap().recoverable);
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .object_deletions_queued,
        0
    );
    repo.restore_trashed_node(
        owner,
        space,
        parent.id,
        entry_version(&db.pool, space, Some(parent.id)).await?,
    )
    .await?;
    let old_deletion_after: Uuid =
        sqlx::query_scalar("SELECT deletion_operation_id FROM nodes WHERE id = $1")
            .bind(old.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(old_deletion_after, old_deletion);
    assert!(repo.find_node(space, old.id).await?.is_none());
    assert!(repo.find_node(space, text.id).await?.is_some());
    assert!(repo.find_node(space, file.id).await?.is_some());
    let bytes: String =
        sqlx::query_scalar("SELECT content_text FROM text_objects WHERE node_id = $1")
            .bind(text.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(bytes, "kept");
    let counters: (i64, i64, i64) = sqlx::query_as("SELECT live_node_count, live_text_bytes, live_file_bytes FROM space_usage WHERE space_id = $1")
        .bind(space).fetch_one(&db.pool).await?;
    assert_eq!(counters, (4, 4, 7));
    let restores: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_change_events WHERE space_id = $1 AND op_type = 'item.restore'",
    )
    .bind(space)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(restores, 1);
    repo.restore_trashed_node(
        owner,
        space,
        old.id,
        entry_version(&db.pool, space, Some(old.id)).await?,
    )
    .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deletion_operations_link_redelete_restore_purge_and_survive_node_cleanup() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-operations").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (item, object) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
    repo.soft_delete_node(space, item.id, owner, false).await?;
    let first = repo.list_trash(owner, 100, None).await?[0]
        .deletion_operation_id
        .unwrap();
    let events = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert_eq!(events[0].op_type, "item.delete");
    assert_eq!(events[0].operation_id, Some(first));

    repo.restore_trashed_node(
        owner,
        space,
        item.id,
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    let cleared: Option<Uuid> =
        sqlx::query_scalar("SELECT deletion_operation_id FROM nodes WHERE id = $1")
            .bind(item.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(cleared, None);
    let events = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert_eq!(events[0].op_type, "item.restore");
    let restored = events[0].operation_id.unwrap();
    assert_ne!(restored, first);
    assert_eq!(
        events[0].metadata["related_deletion_operation_id"],
        first.to_string()
    );

    repo.soft_delete_node(space, item.id, owner, false).await?;
    let second = repo.list_trash(owner, 100, None).await?[0]
        .deletion_operation_id
        .unwrap();
    assert_ne!(second, first);
    assert_ne!(second, restored);
    repo.request_trash_purge(
        owner,
        space,
        Some(item.id),
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    let events = AuditEventRepo::new(db.pool.clone())
        .list_by_owner(owner, 100, None)
        .await?;
    let requested = events
        .iter()
        .find(|e| e.op_type == "trash.purge.request")
        .unwrap();
    assert!(requested.operation_id.is_some());
    assert_ne!(requested.operation_id, Some(second));
    assert_eq!(
        requested.metadata["related_deletion_operation_id"],
        second.to_string()
    );
    PurgeRepo::new(db.pool.clone()).run_once().await?;
    let (node_id, state, linked): (Option<Uuid>, String, Option<Uuid>) = sqlx::query_as(
        "SELECT node_id, state, deletion_operation_id FROM object_storage_objects WHERE object_key = $1",
    ).bind(&object.object_key).fetch_one(&db.pool).await?;
    assert_eq!(node_id, None);
    assert_eq!(state, "delete_pending");
    assert_eq!(linked, Some(second));
    let retained = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert!(retained.iter().any(|e| e.operation_id == Some(first)));
    assert!(retained.iter().any(|e| e.operation_id == Some(second)));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn restore_does_not_depend_on_retained_logs_or_a_legacy_operation_id() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-log-lifetime").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let item = folder(&repo, owner, space, root, "notes").await?;
    repo.soft_delete_node(space, item.id, owner, true).await?;
    let first = repo.list_trash(owner, 100, None).await?[0]
        .deletion_operation_id
        .unwrap();
    sqlx::query("DELETE FROM file_change_events WHERE space_id = $1")
        .bind(space)
        .execute(&db.pool)
        .await?;
    repo.restore_trashed_node(
        owner,
        space,
        item.id,
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    let events = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].metadata["related_deletion_operation_id"],
        first.to_string()
    );

    repo.soft_delete_node(space, item.id, owner, true).await?;
    // A recoverable deletion created before the correlation migration.
    sqlx::query("UPDATE nodes SET deletion_operation_id = NULL WHERE space_id = $1 AND deletion_target_node_id = $2")
        .bind(space).bind(item.id).execute(&db.pool).await?;
    let trash = repo.list_trash(owner, 100, None).await?;
    assert!(trash[0].recoverable);
    assert_eq!(trash[0].deletion_operation_id, None);
    repo.restore_trashed_node(
        owner,
        space,
        item.id,
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    let events = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert_eq!(events[0].op_type, "item.restore");
    assert!(events[0].operation_id.is_some());
    assert!(events[0].metadata["related_deletion_operation_id"].is_null());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn operation_state_and_event_roll_back_together_when_capture_fails() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-operation-rollback").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let item = folder(&repo, owner, space, root, "notes").await?;
    sqlx::query("CREATE FUNCTION reject_trash_event() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'capture unavailable'; END; $$")
        .execute(&db.pool).await?;
    let trigger = "CREATE TRIGGER reject_trash_event BEFORE INSERT ON file_change_events FOR EACH ROW WHEN (NEW.op_type IN ('item.delete', 'item.restore')) EXECUTE FUNCTION reject_trash_event()";
    sqlx::query(trigger).execute(&db.pool).await?;
    assert!(
        repo.soft_delete_node(space, item.id, owner, true)
            .await
            .is_err()
    );
    assert!(repo.find_node(space, item.id).await?.is_some());
    let operation: Option<Uuid> =
        sqlx::query_scalar("SELECT deletion_operation_id FROM nodes WHERE id = $1")
            .bind(item.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(operation, None);
    sqlx::query("DROP TRIGGER reject_trash_event ON file_change_events")
        .execute(&db.pool)
        .await?;
    repo.soft_delete_node(space, item.id, owner, true).await?;
    let deletion = repo.list_trash(owner, 100, None).await?[0]
        .deletion_operation_id
        .unwrap();
    sqlx::query(trigger).execute(&db.pool).await?;
    assert!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await
        .is_err()
    );
    assert!(repo.find_node(space, item.id).await?.is_none());
    assert_eq!(
        repo.list_trash(owner, 100, None).await?[0].deletion_operation_id,
        Some(deletion)
    );
    let events = repo
        .list_file_change_events(space, Some(item.id), 100, None)
        .await?;
    assert_eq!(
        events.iter().filter(|e| e.op_type == "item.delete").count(),
        1
    );
    assert!(!events.iter().any(|e| e.op_type == "item.restore"));
    sqlx::query("DROP TRIGGER reject_trash_event ON file_change_events")
        .execute(&db.pool)
        .await?;
    repo.restore_trashed_node(
        owner,
        space,
        item.id,
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn restore_rejects_wrong_owner_collision_and_current_quota_without_partial_updates()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-guards").await?;
    let stranger = insert_user_account(&db.pool, "trash-stranger", "stranger@example.com").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let item = folder(&repo, owner, space, root, "notes").await?;
    repo.soft_delete_node(space, item.id, owner, true).await?;
    assert!(repo.list_trash(stranger, 100, None).await?.is_empty());
    assert!(matches!(
        repo.restore_trashed_node(
            stranger,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        repo.request_trash_purge(
            stranger,
            space,
            Some(item.id),
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::NotFound(_))
    ));
    let replacement = folder(&repo, owner, space, root, "notes").await?;
    assert!(matches!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::Conflict(_))
    ));
    assert!(repo.find_node(space, replacement.id).await?.is_some());
    repo.soft_delete_node(space, replacement.id, owner, true)
        .await?;
    let limited = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            space_max_nodes: 1,
            ..Limits::default()
        },
    );
    assert!(matches!(
        limited
            .restore_trashed_node(
                owner,
                space,
                item.id,
                entry_version(&db.pool, space, Some(item.id)).await?
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(repo.find_node(space, item.id).await?.is_none());
    repo.restore_trashed_node(
        owner,
        space,
        item.id,
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn restore_time_boundary_is_exclusive_and_injected() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-time").await?;
    let repo = FilesRepo::new(db.pool.clone());
    for (name, offset, succeeds) in [
        ("before", -1, true),
        ("exact", 0, false),
        ("after", 1, false),
    ] {
        let item = folder(&repo, owner, space, root, name).await?;
        let deadline = repo.soft_delete_node(space, item.id, owner, true).await?;
        let timed = repo
            .clone()
            .with_trash_time(deadline + Duration::microseconds(offset));
        let row = timed
            .list_trash(owner, 100, None)
            .await?
            .into_iter()
            .find(|i| i.id == item.id)
            .unwrap();
        assert_eq!(row.recoverable, succeeds);
        assert_eq!(row.deletion_pending, !succeeds);
        assert_eq!(
            timed
                .restore_trashed_node(
                    owner,
                    space,
                    item.id,
                    entry_version(&db.pool, space, Some(item.id)).await?
                )
                .await
                .is_ok(),
            succeeds
        );
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn manual_purge_disables_restore_before_async_cleanup_and_does_not_delete_live_items()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-purge").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (item, _) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
    assert!(matches!(
        repo.request_trash_purge(
            owner,
            space,
            Some(item.id),
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::NotFound(_))
    ));
    repo.soft_delete_node(space, item.id, owner, false).await?;
    repo.request_trash_purge(
        owner,
        space,
        Some(item.id),
        entry_version(&db.pool, space, Some(item.id)).await?,
    )
    .await?;
    assert!(matches!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::Conflict(_))
    ));
    let past: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT deleted_at - interval '1 day' FROM nodes WHERE id = $1")
            .bind(item.id)
            .fetch_one(&db.pool)
            .await?;
    let rolled_back_clock = repo.clone().with_trash_time(past);
    assert!(
        rolled_back_clock
            .restore_trashed_node(
                owner,
                space,
                item.id,
                entry_version(&db.pool, space, Some(item.id)).await?
            )
            .await
            .is_err()
    );
    let queued = rolled_back_clock.list_trash(owner, 100, None).await?;
    assert!(queued[0].deletion_pending);
    assert!(!queued[0].recoverable);
    let before: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE node_id = $1")
            .bind(item.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(
        before, "attached",
        "request is not storage deletion completion"
    );
    let run = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(run.object_deletions_queued, 1);
    let after: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(after, "delete_pending");
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .object_deletions_queued,
        0
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn legacy_and_unavailable_file_content_are_not_resurrected() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-legacy").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (item, _) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
    repo.soft_delete_node(space, item.id, owner, false).await?;
    sqlx::query("UPDATE object_storage_objects SET state = 'delete_pending' WHERE node_id = $1")
        .bind(item.id)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::Conflict(_))
    ));
    sqlx::query("UPDATE nodes SET deletion_target_node_id = NULL WHERE id = $1")
        .bind(item.id)
        .execute(&db.pool)
        .await?;
    let list = repo.list_trash(owner, 100, None).await?;
    assert_eq!(list.len(), 1);
    assert!(!list[0].recoverable);
    assert!(matches!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::Conflict(_))
    ));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_restore_preserves_nodes_but_does_not_restore_previously_deleted_children()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-space").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let old = folder(&repo, owner, space, root, "old").await?;
    repo.soft_delete_node(space, old.id, owner, true).await?;
    let (file, object) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let agent = AgentRepo::new(db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "reader".to_owned(),
            },
            owner,
        )
        .await?;
    ConnectionRepo::new(db.pool.clone())
        .upsert_connection(
            &ConnectAgent {
                space_id: space,
                agent_id: agent.id,
                permission: Permission::Read,
            },
            owner,
        )
        .await?;
    spaces.delete_space(space, owner, owner).await?;
    let list = repo.list_trash(owner, 100, None).await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, "space");
    let first_version = TrashEntryVersion::from(&list[0]);
    let first_deletion = list[0].deletion_operation_id.unwrap();
    repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
        .await?;
    let events = AuditEventRepo::new(db.pool.clone())
        .list_by_owner(owner, 100, None)
        .await?;
    let deleted = events.iter().find(|e| e.op_type == "space.delete").unwrap();
    assert_eq!(deleted.operation_id, Some(first_deletion));
    let restored = events
        .iter()
        .find(|e| e.op_type == "space.restore")
        .unwrap();
    assert!(restored.operation_id.is_some());
    assert_ne!(restored.operation_id, deleted.operation_id);
    assert_eq!(
        restored.metadata["related_deletion_operation_id"],
        first_deletion.to_string()
    );
    let disconnected: bool = sqlx::query_scalar("SELECT disconnected_at IS NOT NULL FROM space_agent_connections WHERE space_id = $1 AND agent_id = $2")
        .bind(space).bind(agent.id).fetch_one(&db.pool).await?;
    assert!(disconnected);
    assert!(repo.find_node(space, file.id).await?.is_some());
    assert!(repo.find_node(space, old.id).await?.is_none());
    assert_eq!(repo.list_trash(owner, 100, None).await?.len(), 1);
    spaces.delete_space(space, owner, owner).await?;
    let second_deletion = repo.list_trash(owner, 100, None).await?[0]
        .deletion_operation_id
        .unwrap();
    assert_ne!(second_deletion, first_deletion);
    assert!(matches!(
        repo.restore_trashed_space(owner, space, first_version)
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        repo.request_trash_purge(owner, space, None, first_version)
            .await,
        Err(Error::Conflict(_))
    ));
    let current = entry_version(&db.pool, space, None).await?;
    assert!(matches!(
        repo.request_trash_purge(
            owner,
            space,
            None,
            TrashEntryVersion {
                deletion_operation_id: None,
                ..current
            }
        )
        .await,
        Err(Error::Conflict(_))
    ));
    repo.request_trash_purge(
        owner,
        space,
        None,
        entry_version(&db.pool, space, None).await?,
    )
    .await?;
    let past: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT deleted_at - interval '1 day' FROM spaces WHERE id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert!(
        repo.clone()
            .with_trash_time(past)
            .restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
            .await
            .is_err()
    );

    assert!(matches!(
        repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .spaces_deleted,
        1
    );
    let ledger_operation: Uuid = sqlx::query_scalar(
        "SELECT deletion_operation_id FROM object_storage_objects WHERE object_key = $1",
    )
    .bind(&object.object_key)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(ledger_operation, second_deletion);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purge_restore_race_never_leaves_a_live_file_queued_for_deletion() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-race").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let (item, file) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
    let deadline = repo.soft_delete_node(space, item.id, owner, false).await? - Duration::days(31);
    sqlx::query("UPDATE nodes SET purge_after = $2 WHERE id = $1")
        .bind(item.id)
        .bind(deadline)
        .execute(&db.pool)
        .await?;
    let restore = repo
        .clone()
        .with_trash_time(deadline - Duration::microseconds(1));
    let purge = PurgeRepo::new(db.pool.clone());
    let expected = entry_version(&db.pool, space, Some(item.id)).await?;
    let (restored, purged) = tokio::join!(
        restore.restore_trashed_node(owner, space, item.id, expected),
        purge.run_once()
    );
    purged?;
    let state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE object_key = $1")
            .bind(&file.object_key)
            .fetch_one(&db.pool)
            .await?;
    if restored.is_ok() {
        assert!(repo.find_node(space, item.id).await?.is_some());
        assert_eq!(state, "attached");
        assert_eq!(purge.run_once().await?.object_deletions_queued, 0);
    } else {
        assert!(repo.find_node(space, item.id).await?.is_none());
        assert_eq!(state, "delete_pending");
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_restore_obeys_current_owner_limits_and_conflicts() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, _) = space_with_root(&db.pool, "trash-space-quota").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let spaces = SpaceRepo::new(db.pool.clone());
    spaces.delete_space(space, owner, owner).await?;
    let replacement = spaces
        .create_space(
            owner,
            &notegate_model::CreateSpace {
                name: "ws-trash-space-quota".to_owned(),
            },
        )
        .await?;
    assert!(matches!(
        repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
            .await,
        Err(Error::Conflict(_))
    ));
    common::set_user_tier(&db.pool, owner, "system_max").await?;
    assert!(
        matches!(
            repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
                .await,
            Err(Error::Conflict(_))
        ),
        "name collision still rejected with spare quota"
    );
    spaces.delete_space(replacement.id, owner, owner).await?;
    repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
        .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn locked_parent_prevents_restore() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-locked-parent").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let parent = folder(&repo, owner, space, root, "parent").await?;
    let item = folder(&repo, owner, space, parent.id, "notes").await?;
    repo.soft_delete_node(space, item.id, owner, true).await?;
    sqlx::query("UPDATE nodes SET write_locked = true WHERE id = $1")
        .bind(parent.id)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        repo.restore_trashed_node(
            owner,
            space,
            item.id,
            entry_version(&db.pool, space, Some(item.id)).await?
        )
        .await,
        Err(Error::WriteLocked { .. })
    ));
    assert!(repo.find_node(space, item.id).await?.is_none());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn restoration_obeys_current_folder_child_limit() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-fanout").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let parent = folder(&repo, owner, space, root, "parent").await?;
    folder(&repo, owner, space, parent.id, "first").await?;
    folder(&repo, owner, space, parent.id, "second").await?;
    repo.soft_delete_node(space, parent.id, owner, true).await?;
    let limited = FilesRepo::with_limits(
        db.pool.clone(),
        Limits {
            folder_max_children: 1,
            ..Limits::default()
        },
    );
    assert!(matches!(
        limited
            .restore_trashed_node(
                owner,
                space,
                parent.id,
                entry_version(&db.pool, space, Some(parent.id)).await?
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(repo.find_node(space, parent.id).await?.is_none());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_restore_rebuilds_links_after_deleted_space_graph_was_cleaned() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-link-rebuild").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let body = StoredContent {
        body: WriteTextBody::Plain("[root](/)".to_owned()),
        content_sha256: "a".repeat(64),
        byte_len: 9,
        line_count: 1,
    };
    let (text, _) = repo
        .insert_text(space, root, "links.md", &body, owner)
        .await?;
    SpaceRepo::new(db.pool.clone())
        .delete_space(space, owner, owner)
        .await?;
    let work = notegate_db::LinkGraphWorkRepo::new(db.pool.clone());
    work.collect_changes().await?;
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM link_graph_space_states WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(before, 0);
    repo.restore_trashed_space(owner, space, entry_version(&db.pool, space, None).await?)
        .await?;
    let pending: bool = sqlx::query_scalar("SELECT available_at IS NOT NULL AND full_scan_event_id IS NOT NULL FROM link_graph_space_states WHERE space_id = $1")
        .bind(space).fetch_one(&db.pool).await?;
    assert!(pending);
    work.collect_changes().await?;
    let scheduled: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM node_link_projections WHERE space_id = $1 AND source_node_id = $2 AND needs_projection AND active_job_id IS NOT NULL)")
        .bind(space).bind(text.id).fetch_one(&db.pool).await?;
    assert!(scheduled);
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn permanent_folder_deletion_includes_independent_trash_but_keeps_its_operation() -> TestResult
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-cascade").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let parent = folder(&repo, owner, space, root, "notes").await?;
    let (old, old_file) = attach_file(&repo, space, parent.id, "old.bin", 3, owner).await?;
    let (current, current_file) =
        attach_file(&repo, space, parent.id, "current.bin", 5, owner).await?;
    repo.soft_delete_node(space, old.id, owner, false).await?;
    let old_version = entry_version(&db.pool, space, Some(old.id)).await?;
    repo.soft_delete_node(space, parent.id, owner, true).await?;
    let parent_version = entry_version(&db.pool, space, Some(parent.id)).await?;
    assert_ne!(
        old_version.deletion_operation_id,
        parent_version.deletion_operation_id
    );
    repo.request_trash_purge(owner, space, Some(parent.id), parent_version)
        .await?;
    let purged = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(purged.object_deletions_queued, 2);
    for (object, expected) in [
        (&old_file.object_key, old_version),
        (&current_file.object_key, parent_version),
    ] {
        let (node, state, operation): (Option<Uuid>, String, Option<Uuid>) = sqlx::query_as("SELECT node_id, state, deletion_operation_id FROM object_storage_objects WHERE object_key = $1")
            .bind(object).fetch_one(&db.pool).await?;
        assert!(node.is_none());
        assert_eq!(state, "delete_pending");
        assert_eq!(operation, expected.deletion_operation_id);
    }
    assert!(repo.find_node(space, current.id).await?.is_none());
    // The now-empty parent is reclaimed in the next batch, not by cascading
    // the whole folder during the first pass.
    let remaining = repo.list_trash(owner, 100, None).await?;
    assert_eq!(remaining.len(), 1);
    assert!(!remaining[0].recoverable);
    let next = PurgeRepo::new(db.pool.clone()).run_once().await?;
    assert_eq!(next.nodes_deleted, 1);
    assert_eq!(next.object_deletions_queued, 0);
    assert!(repo.list_trash(owner, 100, None).await?.is_empty());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn unfinished_upload_can_attach_only_after_its_parent_is_restored() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "trash-pending-upload").await?;
    let repo = FilesRepo::new(db.pool.clone());
    let parent = folder(&repo, owner, space, root, "uploads").await?;
    let upload = Uuid::new_v4();
    repo.insert_object_upload(
        upload,
        &format!("objects/{upload}"),
        space,
        owner,
        &notegate_model::files::BeginObjectUpload {
            parent_node_id: parent.id,
            name: "pending.bin".to_owned(),
            byte_len: 3,
            media_type: "application/octet-stream".to_owned(),
            original_filename: None,
            encryption_mode: notegate_model::FileEncryptionMode::None,
            encryption_metadata: None,
        },
    )
    .await?;
    repo.soft_delete_node(space, parent.id, owner, true).await?;
    assert!(
        repo.attach_object_upload(upload, space, owner, None, None)
            .await
            .is_err()
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM object_storage_objects WHERE id = $1")
            .bind(upload)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(state, "uploading");
    repo.restore_trashed_node(
        owner,
        space,
        parent.id,
        entry_version(&db.pool, space, Some(parent.id)).await?,
    )
    .await?;
    let (node, _) = repo
        .attach_object_upload(upload, space, owner, None, None)
        .await?;
    assert_eq!(node.parent_id, Some(parent.id));
    let count: i64 =
        sqlx::query_scalar("SELECT live_node_count FROM space_usage WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(count, 3);
    db.cleanup().await;
    Ok(())
}
