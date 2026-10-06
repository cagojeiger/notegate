#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::Duration;
use common::{TestDb, attach_file, insert_user_account, space_with_root};
use notegate_core::{Error, limits::Limits};
use notegate_db::{AgentRepo, ConnectionRepo, FilesRepo, PurgeRepo, SpaceRepo};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
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
    let trash = repo.list_trash(owner, 100, None).await?;
    assert_eq!(trash.len(), 2);
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
    repo.restore_trashed_node(owner, space, parent.id).await?;
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
    repo.restore_trashed_node(owner, space, old.id).await?;
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
        repo.restore_trashed_node(stranger, space, item.id).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        repo.request_trash_purge(stranger, space, Some(item.id))
            .await,
        Err(Error::NotFound(_))
    ));
    let replacement = folder(&repo, owner, space, root, "notes").await?;
    assert!(matches!(
        repo.restore_trashed_node(owner, space, item.id).await,
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
        limited.restore_trashed_node(owner, space, item.id).await,
        Err(Error::Conflict(_))
    ));
    assert!(repo.find_node(space, item.id).await?.is_none());
    repo.restore_trashed_node(owner, space, item.id).await?;
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
                .restore_trashed_node(owner, space, item.id)
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
        repo.request_trash_purge(owner, space, Some(item.id)).await,
        Err(Error::NotFound(_))
    ));
    repo.soft_delete_node(space, item.id, owner, false).await?;
    repo.request_trash_purge(owner, space, Some(item.id))
        .await?;
    assert!(matches!(
        repo.restore_trashed_node(owner, space, item.id).await,
        Err(Error::Conflict(_))
    ));
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
        repo.restore_trashed_node(owner, space, item.id).await,
        Err(Error::Conflict(_))
    ));
    sqlx::query("UPDATE nodes SET deletion_root_id = NULL WHERE id = $1")
        .bind(item.id)
        .execute(&db.pool)
        .await?;
    let list = repo.list_trash(owner, 100, None).await?;
    assert_eq!(list.len(), 1);
    assert!(!list[0].recoverable);
    assert!(matches!(
        repo.restore_trashed_node(owner, space, item.id).await,
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
    let (file, _) = attach_file(&repo, space, root, "data.bin", 9, owner).await?;
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
    repo.restore_trashed_space(owner, space).await?;
    let disconnected: bool = sqlx::query_scalar("SELECT disconnected_at IS NOT NULL FROM space_agent_connections WHERE space_id = $1 AND agent_id = $2")
        .bind(space).bind(agent.id).fetch_one(&db.pool).await?;
    assert!(disconnected);
    assert!(repo.find_node(space, file.id).await?.is_some());
    assert!(repo.find_node(space, old.id).await?.is_none());
    assert_eq!(repo.list_trash(owner, 100, None).await?.len(), 1);
    spaces.delete_space(space, owner, owner).await?;
    repo.request_trash_purge(owner, space, None).await?;
    assert!(matches!(
        repo.restore_trashed_space(owner, space).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        PurgeRepo::new(db.pool.clone())
            .run_once()
            .await?
            .spaces_deleted,
        1
    );
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
    let (restored, purged) = tokio::join!(
        restore.restore_trashed_node(owner, space, item.id),
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
        repo.restore_trashed_space(owner, space).await,
        Err(Error::Conflict(_))
    ));
    common::set_user_tier(&db.pool, owner, "system_max").await?;
    assert!(
        matches!(
            repo.restore_trashed_space(owner, space).await,
            Err(Error::Conflict(_))
        ),
        "name collision still rejected with spare quota"
    );
    spaces.delete_space(replacement.id, owner, owner).await?;
    repo.restore_trashed_space(owner, space).await?;
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
    let item = folder(&repo, owner, space, root, "notes").await?;
    repo.soft_delete_node(space, item.id, owner, true).await?;
    sqlx::query("UPDATE nodes SET write_locked = true WHERE id = $1")
        .bind(root)
        .execute(&db.pool)
        .await?;
    assert!(matches!(
        repo.restore_trashed_node(owner, space, item.id).await,
        Err(Error::WriteLocked { .. })
    ));
    assert!(repo.find_node(space, item.id).await?.is_none());
    db.cleanup().await;
    Ok(())
}
