//! Integration tests for `WorkspaceRepo` against a real Postgres schema.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test workspace_repo`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, deactivate_account, insert_user_account};
use notegate_core::{Error, limits};
use notegate_db::{AccessRepo, WorkspaceRepo};
use notegate_model::Role;
use notegate_model::{CreateWorkspace, GrantAccess};
use uuid::Uuid;

async fn make_workspace(repo: &WorkspaceRepo, owner: Uuid, name: &str) -> Uuid {
    repo.create_workspace(
        owner,
        &CreateWorkspace {
            name: name.to_owned(),
        },
    )
    .await
    .expect("workspace insert")
    .id
}

async fn insert_agent_account(
    pool: &sqlx::PgPool,
    creator: Uuid,
    name: &str,
) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar("INSERT INTO accounts (kind) VALUES ('agent') RETURNING id")
        .fetch_one(pool)
        .await?;
    sqlx::query("INSERT INTO agents (id, name, created_by) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(creator)
        .execute(pool)
        .await?;
    Ok(id)
}

/// (a) Creating a workspace materializes exactly one root node, attributed to
/// the creator on both created_by and updated_by (Pre-mortem S1).
#[tokio::test]
async fn create_makes_single_root_node_with_creator_attribution()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;

    let workspace_id = make_workspace(&repo, owner, "personal").await;

    let root_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE workspace_id = $1 AND parent_id IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(root_count, 1, "exactly one root node per workspace");

    let (name, kind, created_by, updated_by): (String, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT name, kind, created_by, updated_by FROM nodes \
         WHERE workspace_id = $1 AND parent_id IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(name, "/");
    assert_eq!(kind, "folder");
    assert_eq!(created_by, owner, "root created_by must be the creator");
    assert_eq!(updated_by, owner, "root updated_by must be the creator");

    // The derived root id is reachable through the store helper.
    let root_id = repo.root_node_id(workspace_id).await?;
    assert!(root_id.is_some());

    db.cleanup().await;
    Ok(())
}

/// (b) Creating a workspace stores an explicit owner access row for the creator.
#[tokio::test]
async fn creator_gets_explicit_owner_access_row() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;

    let workspace_id = make_workspace(&repo, owner, "personal").await;

    let role = repo.role_for(workspace_id, owner).await?;
    assert_eq!(role, Some(Role::Owner));

    let access_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_access          WHERE workspace_id = $1 AND account_id = $2 AND role = 'owner' AND revoked_at IS NULL",
    )
    .bind(workspace_id)
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(access_rows, 1, "creator owner is stored as a live grant");

    // A non-member resolves to no role (treated as 404 by the service layer).
    let stranger = insert_user_account(&db.pool, "stranger", "s@example.test").await?;
    let none = repo.role_for(workspace_id, stranger).await?;
    assert_eq!(none, None);

    db.cleanup().await;
    Ok(())
}

/// (c) The 21st owned workspace is rejected by the in-transaction quota.
#[tokio::test]
async fn twenty_first_owned_workspace_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;

    for index in 0..20 {
        make_workspace(&repo, owner, &format!("ws-{index}")).await;
    }

    let result = repo
        .create_workspace(
            owner,
            &CreateWorkspace {
                name: "ws-overflow".to_owned(),
            },
        )
        .await;
    assert!(
        matches!(result, Err(Error::Conflict(_))),
        "21st owned workspace must be rejected as conflict"
    );

    let owned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspaces WHERE created_by = $1 AND deleted_at IS NULL",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(owned, 20, "the rejected create must not persist");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn inactive_owner_cannot_create_workspace() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    deactivate_account(&db.pool, owner, owner).await?;

    let err = repo
        .create_workspace(
            owner,
            &CreateWorkspace {
                name: "personal".to_owned(),
            },
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, Error::NotFound(message) if message == "workspace owner user account not found")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_account_cannot_create_workspace() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent = insert_agent_account(&db.pool, creator, "bot").await?;

    let err = repo
        .create_workspace(
            agent,
            &CreateWorkspace {
                name: "agent-owned".to_owned(),
            },
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, Error::NotFound(message) if message == "workspace owner user account not found")
    );

    db.cleanup().await;
    Ok(())
}

/// (d) Grant viewer/editor reflects in role_for; revoke clears it; revoked rows
/// do not count toward the active cap, and the 21st active grant is rejected.
#[tokio::test]
async fn grant_revoke_and_access_cap() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;

    let member = insert_user_account(&db.pool, "member", "m@example.test").await?;

    // Grant viewer, then upgrade to editor; role_for reflects each change.
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Viewer,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, member).await?,
        Some(Role::Viewer)
    );

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Editor,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, member).await?,
        Some(Role::Editor)
    );

    // list_access includes the explicit owner row and the member grant.
    let live = access_repo.list_access(workspace_id).await?;
    assert_eq!(live.len(), 2);

    // Revoke clears role_for and drops the row from the live list.
    access_repo
        .revoke_access(workspace_id, member, owner)
        .await?;
    assert_eq!(access_repo.role_for(workspace_id, member).await?, None);
    let live_after = access_repo.list_access(workspace_id).await?;
    assert_eq!(live_after.len(), 1, "only the owner grant remains listed");

    // Re-granting a revoked account succeeds and re-activates the row.
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Viewer,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, member).await?,
        Some(Role::Viewer)
    );

    // Fill the cap: owner + member = 2 active explicit grants; add 18 more for 20 total.
    for index in 0..18 {
        let extra =
            insert_user_account(&db.pool, &format!("extra-{index}"), "e@example.test").await?;
        access_repo
            .upsert_access(
                &GrantAccess {
                    workspace_id,
                    account_id: extra,
                    role: Role::Viewer,
                },
                owner,
            )
            .await?;
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_access WHERE workspace_id = $1 AND revoked_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(active, 20, "cap is reached at 20 active accounts");

    // The 21st distinct active account is rejected.
    let overflow = insert_user_account(&db.pool, "overflow", "x@example.test").await?;
    let result = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: overflow,
                role: Role::Viewer,
            },
            owner,
        )
        .await;
    assert!(
        matches!(result, Err(Error::Conflict(_))),
        "21st active access account must be rejected as conflict"
    );

    // Updating an already-active account at the cap must still succeed (it does
    // not add a new active account).
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Editor,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, member).await?,
        Some(Role::Editor),
        "re-grant of an existing active account is allowed at the cap"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grant_unknown_account_is_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;

    let err = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: Uuid::new_v4(),
                role: Role::Viewer,
            },
            owner,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(message) if message == "account not found"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn inactive_accounts_are_not_live_access() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let member = insert_user_account(&db.pool, "member", "m@example.test").await?;
    let inactive = insert_user_account(&db.pool, "inactive", "i@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Viewer,
            },
            owner,
        )
        .await?;
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: inactive,
                role: Role::Viewer,
            },
            owner,
        )
        .await?;
    deactivate_account(&db.pool, inactive, owner).await?;

    assert_eq!(
        access_repo.role_for(workspace_id, inactive).await?,
        None,
        "inactive account is not a live access grant"
    );
    assert_eq!(
        repo.role_for(workspace_id, inactive).await?,
        None,
        "workspace role lookup must also ignore inactive accounts"
    );
    let live = access_repo.list_access(workspace_id).await?;
    let mut live_ids = live
        .iter()
        .map(|grant| grant.account_id)
        .collect::<Vec<_>>();
    live_ids.sort();
    let mut expected_ids = vec![owner, member];
    expected_ids.sort();
    assert_eq!(
        live_ids, expected_ids,
        "access list includes only active non-revoked accounts"
    );
    assert_eq!(
        repo.list_workspace_views_for(inactive, 100, None)
            .await?
            .len(),
        0,
        "inactive account cannot list workspaces through a stale access row"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn grant_inactive_account_is_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let inactive = insert_user_account(&db.pool, "inactive", "i@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;
    deactivate_account(&db.pool, inactive, owner).await?;

    let err = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: inactive,
                role: Role::Viewer,
            },
            owner,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(message) if message == "account not found"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_account_can_receive_editor_but_owner_grants_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let agent = insert_agent_account(&db.pool, owner, "bot").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: agent,
                role: Role::Editor,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, agent).await?,
        Some(Role::Editor)
    );

    let err = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: agent,
                role: Role::Owner,
            },
            owner,
        )
        .await
        .unwrap_err();

    // P1-5: an owner grant to a non-user (agent) account is a 409 Conflict, not a
    // 400 validation error.
    assert!(
        matches!(err, Error::Conflict(message) if message == "owner role requires an active user account")
    );
    assert_eq!(
        access_repo.role_for(workspace_id, agent).await?,
        Some(Role::Editor),
        "rejected owner grant must leave the previous role unchanged"
    );

    db.cleanup().await;
    Ok(())
}

/// P1-5: an owner grant to an inactive account is a 409 Conflict; a valid owner
/// grant to an active user still succeeds.
#[tokio::test]
async fn owner_grant_to_inactive_account_is_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let inactive = insert_user_account(&db.pool, "inactive", "i@example.test").await?;
    let active = insert_user_account(&db.pool, "active", "a@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "shared").await;
    deactivate_account(&db.pool, inactive, owner).await?;

    let err = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: inactive,
                role: Role::Owner,
            },
            owner,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Conflict(message) if message == "owner role requires an active user account")
    );

    // A valid owner grant to an active user still succeeds.
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: active,
                role: Role::Owner,
            },
            owner,
        )
        .await?;
    assert_eq!(
        access_repo.role_for(workspace_id, active).await?,
        Some(Role::Owner)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn corrupted_agent_owner_row_is_not_an_effective_role()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let agent = insert_agent_account(&db.pool, owner, "bot").await?;
    let workspace_id = make_workspace(&repo, owner, "corrupt").await;

    sqlx::query(
        "INSERT INTO workspace_access (workspace_id, account_id, role, granted_by) \
         VALUES ($1, $2, 'owner', $3)",
    )
    .bind(workspace_id)
    .bind(agent)
    .bind(owner)
    .execute(&db.pool)
    .await?;

    assert_eq!(access_repo.role_for(workspace_id, agent).await?, None);
    assert_eq!(repo.role_for(workspace_id, agent).await?, None);
    assert!(
        repo.find_workspace_view_for(agent, workspace_id)
            .await?
            .is_none(),
        "agent owner rows are hidden from workspace visibility"
    );
    assert!(
        access_repo
            .list_access(workspace_id)
            .await?
            .iter()
            .all(|grant| grant.account_id != agent),
        "agent owner rows are hidden from access listings"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn user_account_can_receive_owner_access() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let member = insert_user_account(&db.pool, "member", "m@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "owned").await;

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Owner,
            },
            owner,
        )
        .await?;

    assert_eq!(repo.role_for(workspace_id, owner).await?, Some(Role::Owner));
    assert_eq!(
        access_repo.role_for(workspace_id, member).await?,
        Some(Role::Owner)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn creator_owner_access_cannot_be_revoked_or_downgraded()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let access_repo = AccessRepo::new(db.pool.clone());
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "owned").await;

    let downgrade = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: owner,
                role: Role::Editor,
            },
            owner,
        )
        .await
        .unwrap_err();
    assert!(matches!(downgrade, Error::Conflict(_)));

    let revoke = access_repo
        .revoke_access(workspace_id, owner, owner)
        .await
        .unwrap_err();
    assert!(matches!(revoke, Error::Conflict(_)));
    assert_eq!(
        access_repo.role_for(workspace_id, owner).await?,
        Some(Role::Owner)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn last_active_user_owner_cannot_be_revoked() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let co_owner = insert_user_account(&db.pool, "co-owner", "co@example.test").await?;
    let workspace_id = make_workspace(&repo, creator, "owned").await;

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: co_owner,
                role: Role::Owner,
            },
            creator,
        )
        .await?;
    deactivate_account(&db.pool, creator, creator).await?;

    let revoke = access_repo
        .revoke_access(workspace_id, co_owner, co_owner)
        .await
        .unwrap_err();
    assert!(matches!(revoke, Error::Conflict(_)));
    assert_eq!(
        access_repo.role_for(workspace_id, co_owner).await?,
        Some(Role::Owner)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn revoking_member_grant_does_not_affect_owner() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let member = insert_user_account(&db.pool, "member", "m@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "owned").await;

    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member,
                role: Role::Editor,
            },
            owner,
        )
        .await?;
    access_repo
        .revoke_access(workspace_id, member, owner)
        .await?;

    assert_eq!(repo.role_for(workspace_id, owner).await?, Some(Role::Owner));
    assert_eq!(access_repo.role_for(workspace_id, member).await?, None);

    db.cleanup().await;
    Ok(())
}

/// (e) A duplicate live `(created_by, name)` surfaces as a clean error, not a
/// raw internal failure.
#[tokio::test]
async fn duplicate_owner_name_is_a_clean_error() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;

    make_workspace(&repo, owner, "personal").await;

    let result = repo
        .create_workspace(
            owner,
            &CreateWorkspace {
                name: "personal".to_owned(),
            },
        )
        .await;

    match result {
        Err(notegate_core::Error::Conflict(_)) => {}
        other => panic!("expected a conflict error on duplicate name, got {other:?}"),
    }

    // The same name is fine for a different owner (name is not global-unique).
    let other_owner = insert_user_account(&db.pool, "other", "o2@example.test").await?;
    assert!(
        repo.create_workspace(
            other_owner,
            &CreateWorkspace {
                name: "personal".to_owned(),
            },
        )
        .await
        .is_ok()
    );

    db.cleanup().await;
    Ok(())
}

/// rename and delete round-trip at the repo level (owner-gated in the service).
#[tokio::test]
async fn rename_and_delete_workspace() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let workspace_id = make_workspace(&repo, owner, "before").await;

    let renamed = repo.rename_workspace(workspace_id, "after").await?;
    assert_eq!(renamed.name, "after");

    repo.delete_workspace(workspace_id, owner).await?;
    assert!(repo.find_workspace(workspace_id).await?.is_none());

    let missing = repo.delete_workspace(Uuid::new_v4(), owner).await;
    assert!(
        matches!(missing, Err(Error::NotFound(message)) if message == "workspace not found"),
        "deleting a missing workspace must be a clean not-found"
    );

    // The cascade removed the root node too.
    let nodes: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        nodes, 1,
        "soft-deleting a workspace keeps nodes until purge"
    );

    db.cleanup().await;
    Ok(())
}

/// list_workspaces_for returns only workspaces with a live grant for the caller.
#[tokio::test]
async fn list_workspaces_for_filters_to_live_grants() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "o@example.test").await?;
    let member = insert_user_account(&db.pool, "member", "m@example.test").await?;

    let ws1 = make_workspace(&repo, owner, "one").await;
    let _ws2 = make_workspace(&repo, owner, "two").await;

    // Owner sees both; member sees none yet.
    assert_eq!(
        repo.list_workspace_views_for(owner, 100, None).await?.len(),
        2
    );
    assert_eq!(
        repo.list_workspace_views_for(member, 100, None)
            .await?
            .len(),
        0
    );

    // Grant then revoke on ws1: member sees it, then stops seeing it.
    access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id: ws1,
                account_id: member,
                role: Role::Viewer,
            },
            owner,
        )
        .await?;
    assert_eq!(
        repo.list_workspace_views_for(member, 100, None)
            .await?
            .len(),
        1
    );

    access_repo.revoke_access(ws1, member, owner).await?;
    assert_eq!(
        repo.list_workspace_views_for(member, 100, None)
            .await?
            .len(),
        0,
        "revoked grant excludes the workspace from the listing"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn account_accessible_workspace_cap_is_enforced() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = WorkspaceRepo::new(db.pool.clone());
    let access_repo = AccessRepo::new(db.pool.clone());
    let member = insert_user_account(&db.pool, "member-cap", "member-cap@example.test").await?;

    for index in 0..limits::ACCESSIBLE_WORKSPACES_PER_ACCOUNT_MAX {
        let owner = insert_user_account(
            &db.pool,
            &format!("owner-cap-{index}"),
            &format!("owner-cap-{index}@example.test"),
        )
        .await?;
        let workspace_id = make_workspace(&repo, owner, &format!("ws-{index}")).await;
        access_repo
            .upsert_access(
                &GrantAccess {
                    workspace_id,
                    account_id: member,
                    role: Role::Viewer,
                },
                owner,
            )
            .await?;
    }

    let owner =
        insert_user_account(&db.pool, "owner-cap-over", "owner-cap-over@example.test").await?;
    let overflow_workspace = make_workspace(&repo, owner, "overflow").await;
    let result = access_repo
        .upsert_access(
            &GrantAccess {
                workspace_id: overflow_workspace,
                account_id: member,
                role: Role::Viewer,
            },
            owner,
        )
        .await;

    assert!(
        matches!(result, Err(Error::Conflict(_))),
        "grant over account accessible workspace cap must be rejected"
    );

    db.cleanup().await;
    Ok(())
}
