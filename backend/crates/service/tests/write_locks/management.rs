use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, SpaceRepo};
use notegate_model::{
    AccountKind, Caller, CallerIdentity, Channel, ConnectAgent, CreateAgent, Permission,
};
use notegate_service::ServiceError;
use notegate_service::connections::ConnectionService;
use notegate_service::files::{UpdateNodeWriteLock, WriteTarget, WriteText, WriteTextBody};
use notegate_service::spaces::SpaceService;

use crate::common::insert_user_account;
use crate::write_lock_support::{
    Fixture, TestResult, assert_forbidden, assert_write_locked, load_caller,
};

#[tokio::test]
async fn lock_management_is_browser_owner_only_and_tier_gated() -> TestResult {
    let Some(fixture) = Fixture::setup("policy").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "policy.md").await?;
    let other =
        insert_user_account(&fixture.db.pool, "write-lock-other", "other@example.test").await?;

    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &load_caller(&fixture.db, fixture.owner, Channel::Mcp).await,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                    expected_revision: crate::common::node_revision(&fixture.db.pool, text_id)
                        .await?,
                },
            )
            .await,
    );
    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &load_caller(&fixture.db, other, Channel::Browser).await,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                    expected_revision: crate::common::node_revision(&fixture.db.pool, text_id)
                        .await?,
                },
            )
            .await,
    );

    let agent = AgentRepo::new(fixture.db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "write-lock-agent".to_owned(),
            },
            fixture.owner,
        )
        .await?;
    let agent_id = agent.id;
    ConnectionService::new(ConnectionRepo::new(fixture.db.pool.clone()))
        .connect(
            AccountKind::User,
            fixture.owner,
            ConnectAgent {
                space_id: fixture.space_id,
                agent_id,
                permission: Permission::Write,
            },
        )
        .await?;
    let agent_account = AccountRepo::new(fixture.db.pool.clone())
        .find_account(agent_id)
        .await?
        .expect("agent account");
    assert_forbidden(
        fixture
            .files
            .update_node_write_lock(
                &Caller {
                    account: agent_account,
                    identity: CallerIdentity::Agent(agent),
                    channel: Channel::Browser,
                },
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                    expected_revision: crate::common::node_revision(&fixture.db.pool, text_id)
                        .await?,
                },
            )
            .await,
    );

    assert!(matches!(
        fixture
            .files
            .update_node_write_lock(
                &fixture.browser,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: fixture.root_id,
                    enabled: true,
                    expected_revision: crate::common::node_revision(
                        &fixture.db.pool,
                        fixture.root_id,
                    )
                    .await?,
                },
            )
            .await,
        Err(ServiceError::Conflict(_))
    ));

    fixture.set_lock(text_id, true).await?;
    assert_write_locked(
        fixture
            .files
            .write_text(
                agent_id,
                fixture.space_id,
                WriteText {
                    target: WriteTarget::Existing { node_id: text_id },
                    body: WriteTextBody::Plain("agent write".to_owned()),
                    expected_revision: Some(
                        crate::common::node_revision(&fixture.db.pool, text_id).await?,
                    ),
                    expected_sha256: None,
                },
            )
            .await,
    );
    sqlx::query("UPDATE users SET tier = 'tier0' WHERE id = $1")
        .bind(fixture.owner)
        .execute(&fixture.db.pool)
        .await?;
    fixture.set_lock(text_id, false).await?;
    assert!(matches!(
        fixture
            .files
            .update_node_write_lock(
                &fixture.browser,
                fixture.space_id,
                UpdateNodeWriteLock {
                    node_id: text_id,
                    enabled: true,
                    expected_revision: crate::common::node_revision(&fixture.db.pool, text_id)
                        .await?,
                },
            )
            .await,
        Err(ServiceError::Conflict(_))
    ));

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_lock_and_write_are_serialized() -> TestResult {
    let Some(fixture) = Fixture::setup("race").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "race.md").await?;
    let before_event_id: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(id), 0) FROM file_change_events")
            .fetch_one(&fixture.db.pool)
            .await?;
    let expected_revision = crate::common::node_revision(&fixture.db.pool, text_id).await?;

    let write = fixture.files.write_text(
        fixture.owner,
        fixture.space_id,
        WriteText {
            target: WriteTarget::Existing { node_id: text_id },
            body: WriteTextBody::Plain("concurrent write".to_owned()),
            expected_revision: Some(expected_revision),
            expected_sha256: None,
        },
    );
    let lock = fixture.files.update_node_write_lock(
        &fixture.browser,
        fixture.space_id,
        UpdateNodeWriteLock {
            node_id: text_id,
            enabled: true,
            expected_revision,
        },
    );
    let (write_result, lock_result) = tokio::join!(write, lock);

    let events: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT op_type, metadata FROM file_change_events \
         WHERE space_id = $1 AND node_id = $2 AND id > $3 ORDER BY id",
    )
    .bind(fixture.space_id)
    .bind(text_id)
    .bind(before_event_id)
    .fetch_all(&fixture.db.pool)
    .await?;
    match (write_result, lock_result) {
        (Ok(_), Err(ServiceError::Conflict(_))) => {
            assert_eq!(
                events.iter().filter(|(op, _)| op == "text.write").count(),
                1
            );
            assert!(
                !fixture
                    .files
                    .stat(fixture.owner, fixture.space_id, text_id)
                    .await?
                    .node
                    .write_locked
            );
        }
        (Err(ServiceError::WriteLocked { .. }), Ok(_)) => {
            assert_eq!(
                events
                    .iter()
                    .filter(|(_, metadata)| metadata["write_lock_changed"] == true)
                    .count(),
                1
            );
            assert!(
                fixture
                    .files
                    .stat(fixture.owner, fixture.space_id, text_id)
                    .await?
                    .node
                    .write_locked
            );
        }
        (write, lock) => panic!("unexpected race results: write={write:?}, lock={lock:?}"),
    }

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_deletion_remains_an_owner_lifecycle_action() -> TestResult {
    let Some(fixture) = Fixture::setup("space-delete").await? else {
        return Ok(());
    };
    let text_id = fixture.text(fixture.root_id, "locked.md").await?;
    fixture.set_lock(text_id, true).await?;
    let expected_revision = crate::common::node_revision(&fixture.db.pool, text_id).await?;

    SpaceService::new(SpaceRepo::new(fixture.db.pool.clone()))
        .delete(AccountKind::User, fixture.owner, fixture.space_id)
        .await?;
    assert!(
        SpaceRepo::new(fixture.db.pool.clone())
            .find_space(fixture.space_id)
            .await?
            .is_none()
    );
    assert!(matches!(
        fixture
            .files
            .write_text(
                fixture.owner,
                fixture.space_id,
                WriteText {
                    target: WriteTarget::Existing { node_id: text_id },
                    body: WriteTextBody::Plain("blocked by deleted space".to_owned()),
                    expected_revision: Some(expected_revision),
                    expected_sha256: None,
                },
            )
            .await,
        Err(ServiceError::NotFound(_))
    ));

    fixture.cleanup().await;
    Ok(())
}
