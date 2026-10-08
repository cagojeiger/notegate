#![allow(
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
mod common;
use chrono::{DateTime, Duration, Utc};
use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, FilesRepo, SpaceRepo, files::revisions};
use notegate_model::{AccountKind, Channel, ConnectAgent, CreateAgent, Permission};
use notegate_service::{
    ServiceError,
    connections::ConnectionService,
    files::{
        CreateText, DeleteNode, FilesService, ReadText, ReadTextBody,
        UpdateNodeExternalAccessPolicy, UpdateNodeWriteLock, UpdateTextEncryption, WriteTarget,
        WriteText, WriteTextBody,
    },
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn expired_revision_restore_leaves_current_document_readable_and_writable() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "expired-revision", "expired-revision@example.com").await?;
    let (space, root) = setup_space(&SpaceRepo::new(db.pool.clone()), owner, "history").await;
    let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse()?;
    let session = Uuid::new_v4();
    let repo = FilesRepo::new(db.pool.clone());
    let files_at = |time| {
        FilesService::new(repo.clone().with_revision_time(time))
            .with_revision_session(Some(session))
    };
    let created = files_at(now)
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "note.md".into(),
            },
        )
        .await?;
    let node = created.node.node.id;
    for (second, content) in [(1, "intermediate"), (2, "current")] {
        files_at(now + Duration::seconds(second))
            .write_text(
                owner,
                space,
                WriteText {
                    target: WriteTarget::Existing { node_id: node },
                    body: WriteTextBody::Plain(content.into()),
                    expected_sha256: None,
                },
            )
            .await?;
    }
    let expired: Uuid =
        sqlx::query_scalar("SELECT id FROM text_revisions WHERE node_id = $1 AND NOT checkpoint")
            .bind(node)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(
        revisions::cleanup_at(&db.pool, now + Duration::days(2)).await?,
        1
    );
    let files = files_at(now + Duration::days(2));
    let history = files.text_revisions(owner, space, node, 10, None).await?;
    assert_eq!(
        history.revisions.len(),
        1,
        "the checkpoint remains available"
    );
    assert!(
        !history
            .revisions
            .iter()
            .any(|revision| revision.id == expired)
    );
    let current_id: Uuid =
        sqlx::query_scalar("SELECT revision_id FROM text_objects WHERE node_id = $1")
            .bind(node)
            .fetch_one(&db.pool)
            .await?;
    let changes = repo
        .list_file_change_events(space, Some(node), 20, None)
        .await?;
    let write = changes
        .iter()
        .find(|event| event.metadata["before_revision_id"] == expired.to_string())
        .unwrap();
    assert_eq!(write.metadata["before_revision_status"], "deleted");
    assert_eq!(
        write.metadata["before_revision_deletion_reason"],
        "intermediate_expired"
    );
    assert_eq!(write.metadata["after_revision_status"], "current");
    let read_request = || ReadText {
        node_id: node,
        start_line: None,
        max_lines: None,
        max_bytes: None,
        if_none_match_sha256: None,
    };
    let before = files.read_text(owner, space, read_request()).await?;
    assert!(matches!(&before.body, ReadTextBody::Content(body) if body.content == "current"));
    assert!(matches!(
        files
            .restore_text_revision(owner, space, node, expired, before.content_sha256.clone())
            .await,
        Err(ServiceError::NotFound(_))
    ));
    let after = files.read_text(owner, space, read_request()).await?;
    assert_eq!(after.content_sha256, before.content_sha256);
    assert!(matches!(&after.body, ReadTextBody::Content(body) if body.content == "current"));
    let unchanged = files.text_revisions(owner, space, node, 10, None).await?;
    let unchanged_id: Uuid =
        sqlx::query_scalar("SELECT revision_id FROM text_objects WHERE node_id = $1")
            .bind(node)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(unchanged_id, current_id);
    assert_eq!(unchanged.revisions.len(), history.revisions.len());
    assert_eq!(
        repo.list_file_change_events(space, Some(node), 20, None)
            .await?
            .len(),
        changes.len()
    );
    let written = files
        .write_text(
            owner,
            space,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                body: WriteTextBody::Plain("after expiration".into()),
                expected_sha256: Some(after.content_sha256),
            },
        )
        .await?;
    let read = files.read_text(owner, space, read_request()).await?;
    assert_eq!(read.content_sha256, written.text.content_sha256);
    assert!(matches!(read.body, ReadTextBody::Content(body) if body.content == "after expiration"));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn history_obeys_permissions_external_policy_write_locks_and_deletion() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "revision-owner", "revision-owner@example.com").await?;
    // Two Spaces are needed to exercise cross-Space ID scoping.
    sqlx::query("UPDATE users SET tier='system_max' WHERE id=$1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    let stranger = insert_user_account(
        &db.pool,
        "revision-stranger",
        "revision-stranger@example.com",
    )
    .await?;
    let (space, root) = setup_space(&SpaceRepo::new(db.pool.clone()), owner, "history").await;
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let (account, user) = AccountRepo::new(db.pool.clone())
        .find_caller_by_account_id(owner)
        .await?
        .unwrap();
    let browser = notegate_model::Caller {
        account,
        identity: notegate_model::CallerIdentity::User(user),
        channel: Channel::Browser,
    };

    let created = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "note.md".into(),
            },
        )
        .await?;
    let node = created.node.node.id;
    let current = files
        .write_text(
            owner,
            space,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                body: WriteTextBody::Plain("current".into()),
                expected_sha256: None,
            },
        )
        .await?;
    let page = files.text_revisions(owner, space, node, 10, None).await?;
    let id = page.revisions[0].id;
    assert!(matches!(
        files.text_revision(stranger, space, node, id).await,
        Err(ServiceError::NotFound(_))
    ));
    assert!(matches!(
        files.text_revision(owner, space, Uuid::new_v4(), id).await,
        Err(ServiceError::NotFound(_))
    ));
    let (other_space, _) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "other-history").await;
    assert!(matches!(
        files.text_revision(owner, other_space, node, id).await,
        Err(ServiceError::NotFound(_))
    ));
    let agent = AgentRepo::new(db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "reader".into(),
            },
            owner,
        )
        .await?
        .id;
    ConnectionService::new(ConnectionRepo::new(db.pool.clone()))
        .connect(
            AccountKind::User,
            owner,
            ConnectAgent {
                space_id: space,
                agent_id: agent,
                permission: Permission::Read,
            },
        )
        .await?;
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id: node,
                enabled: true,
            },
        )
        .await?;
    let api = files.for_channel(Channel::Api);
    assert!(api.text_revision(agent, space, node, id).await.is_ok());
    assert!(matches!(
        api.restore_text_revision(agent, space, node, id, current.text.content_sha256.clone())
            .await,
        Err(ServiceError::Forbidden(_))
    ));
    files
        .update_node_external_access_policy(
            AccountKind::User,
            owner,
            space,
            UpdateNodeExternalAccessPolicy {
                node_id: node,
                enabled: false,
            },
        )
        .await?;
    assert!(matches!(
        api.text_revision(agent, space, node, id).await,
        Err(ServiceError::NotFound(_))
    ));
    files
        .update_node_write_lock(
            &browser,
            space,
            UpdateNodeWriteLock {
                node_id: node,
                enabled: true,
            },
        )
        .await?;
    assert!(matches!(
        files
            .restore_text_revision(owner, space, node, id, current.text.content_sha256.clone())
            .await,
        Err(ServiceError::WriteLocked { .. })
    ));
    files
        .update_node_write_lock(
            &browser,
            space,
            UpdateNodeWriteLock {
                node_id: node,
                enabled: false,
            },
        )
        .await?;
    files
        .delete_node(
            owner,
            space,
            DeleteNode {
                node_id: node,
                recursive: false,
            },
        )
        .await?;
    assert!(matches!(
        files.text_revision(owner, space, node, id).await,
        Err(ServiceError::NotFound(_))
    ));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn server_encryption_changes_do_not_create_versions_or_change_body_attribution() -> TestResult
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "revision-encryption",
        "revision-encryption@example.com",
    )
    .await?;
    // The encryption feature is gated by the existing owner's tier.
    sqlx::query("UPDATE users SET tier='system_max' WHERE id=$1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    let (space, root) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "encrypted-history").await;
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let created = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "note.md".into(),
            },
        )
        .await?;
    let node = created.node.node.id;
    let first = files
        .write_text(
            owner,
            space,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                body: WriteTextBody::Plain("secret-one".into()),
                expected_sha256: None,
            },
        )
        .await?;
    let written_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT revision_written_at FROM text_objects WHERE node_id=$1")
            .bind(node)
            .fetch_one(&db.pool)
            .await?;
    files
        .update_text_encryption(
            AccountKind::User,
            owner,
            space,
            UpdateTextEncryption {
                node_id: node,
                enabled: true,
            },
        )
        .await?;
    files
        .write_text(
            owner,
            space,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                body: WriteTextBody::Plain("secret-two".into()),
                expected_sha256: Some(first.text.content_sha256),
            },
        )
        .await?;
    files
        .update_text_encryption(
            AccountKind::User,
            owner,
            space,
            UpdateTextEncryption {
                node_id: node,
                enabled: false,
            },
        )
        .await?;
    let page = files.text_revisions(owner, space, node, 10, None).await?;
    assert_eq!(page.revisions.len(), 2);
    assert_eq!(page.revisions[0].written_at, written_at);
    assert_eq!(page.revisions[0].author_id, owner);
    assert_eq!(
        files
            .text_revision(owner, space, node, page.revisions[0].id)
            .await?
            .content,
        "secret-one"
    );
    let decrypted_current = files
        .write_text(
            owner,
            space,
            WriteText {
                target: WriteTarget::Existing { node_id: node },
                body: WriteTextBody::Plain("secret-three".into()),
                expected_sha256: None,
            },
        )
        .await?;
    let restored = files
        .restore_text_revision(
            owner,
            space,
            node,
            page.revisions[0].id,
            decrypted_current.text.content_sha256,
        )
        .await?;
    assert_eq!(restored.text.content, Some("secret-one".into()));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cursor_is_document_bound_and_opaque_current_content_hides_history() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "revision-cursor", "revision-cursor@example.com").await?;
    let (space, root) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "cursor-history").await;
    let files = FilesService::new(FilesRepo::new(db.pool.clone()));
    let node = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "note.md".into(),
            },
        )
        .await?
        .node
        .node
        .id;
    let other = files
        .create_text(
            owner,
            space,
            CreateText {
                parent_node_id: root,
                name: "other.md".into(),
            },
        )
        .await?
        .node
        .node
        .id;
    for value in ["one", "two"] {
        files
            .write_text(
                owner,
                space,
                WriteText {
                    target: WriteTarget::Existing { node_id: node },
                    body: WriteTextBody::Plain(value.into()),
                    expected_sha256: None,
                },
            )
            .await?;
    }
    let page = files.text_revisions(owner, space, node, 1, None).await?;
    assert!(page.next_cursor.is_some());
    assert!(matches!(
        files
            .text_revisions(owner, space, other, 1, page.next_cursor.as_deref())
            .await,
        Err(ServiceError::InvalidInput(_))
    ));
    sqlx::query("UPDATE text_objects SET storage_format='encrypted',content_text=NULL,encrypted_payload='{}'::jsonb WHERE node_id=$1").bind(node).execute(&db.pool).await?;
    assert!(matches!(
        files.text_revisions(owner, space, node, 10, None).await,
        Err(ServiceError::InvalidInput(_))
    ));
    assert!(matches!(
        files
            .text_revision(owner, space, node, page.revisions[0].id)
            .await,
        Err(ServiceError::InvalidInput(_))
    ));
    db.cleanup().await;
    Ok(())
}
