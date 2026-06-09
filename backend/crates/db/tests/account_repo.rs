//! Integration tests for `AccountRepo` against a real Postgres schema.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test account_repo`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::TestDb;
use notegate_db::{AccessRepo, AccountRepo, AgentRepo};
use notegate_model::{CreateAgent, CreateAgentKey, GrantAccess, ResolveAttrs, Role};
use sqlx::Row as _;

fn attrs(sub: &str, email: &str, name: &str) -> ResolveAttrs {
    ResolveAttrs {
        sub: sub.to_owned(),
        email: email.to_owned(),
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn upsert_user_creates_account_and_user_rows() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, user) = repo
        .upsert_user_by_sub(&attrs("sub-1", "a@example.test", "Kang"))
        .await?;
    assert_eq!(account.id, user.id);
    assert_eq!(account.kind.as_str(), "user");
    assert_eq!(account.display_name, "Kang");
    assert!(account.is_active);
    assert_eq!(user.email.as_deref(), Some("a@example.test"));

    let plaintext_matches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM accounts \
         WHERE display_name_ciphertext::text LIKE '%Kang%'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(plaintext_matches, 0);

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(accounts, 1);
    assert_eq!(users, 1);
    let workspace_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        workspace_count, 0,
        "new user does not auto-create a workspace"
    );
    let access_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workspace_access")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(access_count, 0, "new user does not auto-create access rows");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn duplicate_sub_updates_and_does_not_duplicate() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (first, _) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "old@example.test", "Old Name"))
        .await?;
    let (second, user) = repo
        .upsert_user_by_sub(&attrs("sub-dup", "new@example.test", "New Name"))
        .await?;

    assert_eq!(first.id, second.id, "same sub must reuse the same account");
    assert_eq!(second.display_name, "New Name");
    assert_eq!(user.email.as_deref(), Some("new@example.test"));

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(
        accounts, 1,
        "duplicate sub must not create a second account"
    );
    let workspaces: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(workspaces, 0, "duplicate sub must not create workspaces");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn find_user_by_sub_and_account_resolve_the_pair() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (created, _) = repo
        .upsert_user_by_sub(&attrs("sub-find", "f@example.test", "Finder"))
        .await?;

    let (account, user) = repo
        .find_user_by_sub("sub-find")
        .await?
        .ok_or("user resolves by sub")?;
    assert_eq!(account.id, created.id);
    assert_eq!(user.email.as_deref(), Some("f@example.test"));

    let by_id = repo.find_caller_by_account_id(created.id).await?;
    assert_eq!(by_id.map(|(a, _)| a.id), Some(created.id));

    let by_account = repo.find_account(created.id).await?;
    assert_eq!(by_account.map(|a| a.id), Some(created.id));

    assert!(repo.find_user_by_sub("nope").await?.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn anonymize_user_clears_pii_and_deactivates() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AccountRepo::new(db.pool.clone());

    let (account, _) = repo
        .upsert_user_by_sub(&attrs("sub-gone", "g@example.test", "Gone"))
        .await?;

    repo.anonymize_user(account.id, account.id).await?;

    let after = repo
        .find_account(account.id)
        .await?
        .ok_or("account still present after soft delete")?;
    assert!(!after.is_active);
    assert!(after.deleted_at.is_some());
    assert_eq!(after.deleted_by, Some(account.id));

    let row = sqlx::query(
        "SELECT provider_sub_hash, email_hash, email_ciphertext, anonymized_at \
         FROM users WHERE id = $1",
    )
    .bind(account.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(row.get::<Option<String>, _>("provider_sub_hash").is_none());
    assert!(row.get::<Option<String>, _>("email_hash").is_none());
    assert!(row.get::<Option<Vec<u8>>, _>("email_ciphertext").is_none());
    assert!(
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("anonymized_at")
            .is_some()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn anonymize_user_soft_deletes_owned_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let accounts = AccountRepo::new(db.pool.clone());
    let agents = AgentRepo::new(db.pool.clone());
    let access = AccessRepo::new(db.pool.clone());

    let (owner, _) = accounts
        .upsert_user_by_sub(&attrs("owner-sub", "owner@example.test", "Owner"))
        .await?;
    let (member, _) = accounts
        .upsert_user_by_sub(&attrs("member-sub", "member@example.test", "Member"))
        .await?;

    let workspace_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name) VALUES ($1, 'owned') RETURNING id",
    )
    .bind(owner.id)
    .fetch_one(&db.pool)
    .await?;

    let agent = agents
        .insert_agent(
            &CreateAgent {
                name: "owned-agent".to_owned(),
            },
            owner.id,
        )
        .await?;
    let key = agents
        .insert_agent_key(
            &CreateAgentKey {
                agent_id: agent.id,
                name: "key".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            "hashed-token",
            owner.id,
        )
        .await?;

    access
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: member.id,
                role: Role::Viewer,
            },
            owner.id,
        )
        .await?;
    access
        .upsert_access(
            &GrantAccess {
                workspace_id,
                account_id: agent.id,
                role: Role::Editor,
            },
            owner.id,
        )
        .await?;

    accounts.anonymize_user(owner.id, owner.id).await?;

    let account = accounts
        .find_account(owner.id)
        .await?
        .ok_or("owner account remains as attribution target")?;
    assert!(!account.is_active);
    assert_eq!(account.display_name, "");
    assert!(accounts.find_user_by_sub("owner-sub").await?.is_none());

    let workspace_deleted: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<uuid::Uuid>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as("SELECT deleted_at, deleted_by, purge_after FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .fetch_one(&db.pool)
        .await?;
    assert!(workspace_deleted.0.is_some());
    assert_eq!(workspace_deleted.1, Some(owner.id));
    assert!(workspace_deleted.2.is_some());

    let agent_active: bool = sqlx::query_scalar("SELECT is_active FROM accounts WHERE id = $1")
        .bind(agent.id)
        .fetch_one(&db.pool)
        .await?;
    assert!(!agent_active);

    let key_revoked_by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT revoked_by FROM agent_keys WHERE id = $1")
            .bind(key.id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(key_revoked_by, Some(owner.id));

    let live_access: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_access WHERE workspace_id = $1 AND revoked_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(live_access, 0);

    let key_destroyed: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT destroyed_at FROM account_encryption_keys WHERE account_id = $1",
    )
    .bind(owner.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(key_destroyed.is_some());

    db.cleanup().await;
    Ok(())
}
