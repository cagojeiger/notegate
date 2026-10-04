#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod common;
use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, FilesRepo, SpaceRepo};
use notegate_model::{AccountKind, Channel, ConnectAgent, CreateAgent, Permission};
use notegate_service::{
    ServiceError,
    connections::ConnectionService,
    files::{
        CreateText, DeleteNode, FilesService, UpdateNodeExternalAccessPolicy, UpdateNodeWriteLock,
        UpdateTextEncryption, WriteTarget, WriteText, WriteTextBody,
    },
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn history_obeys_permissions_external_policy_write_locks_and_deletion() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "revision-owner", "revision-owner@example.com").await?;
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
