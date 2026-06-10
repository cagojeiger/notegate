//! Integration tests for `AgentRepo` against a real Postgres schema.
//!
//! Run with:
//! `NOTEGATE_TEST_DATABASE_URL=postgres://notegate:notegate@localhost:5433/notegate \
//!  cargo test -p notegate-db --test agent_repo`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account};
use notegate_core::{Error, limits};
use notegate_db::{AgentRepo, ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::{CreateAgent, CreateApiKey};
use uuid::Uuid;

async fn make_agent(repo: &AgentRepo, creator: Uuid, name: &str) -> Uuid {
    repo.insert_agent(
        &CreateAgent {
            name: name.to_owned(),
        },
        creator,
    )
    .await
    .expect("agent insert")
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

#[tokio::test]
async fn create_agent_writes_account_and_attribution() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;

    let agent = repo
        .insert_agent(
            &CreateAgent {
                name: "research-agent".to_owned(),
            },
            creator,
        )
        .await?;
    assert_eq!(agent.name, "research-agent");
    assert_eq!(agent.created_by, creator);

    let kind: String = sqlx::query_scalar("SELECT kind FROM accounts WHERE id = $1")
        .bind(agent.id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(kind, "agent");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_agent_rejects_blank_or_overlong_name() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;

    for name in [
        "   ".to_owned(),
        "a".repeat(limits::AGENT_NAME_MAX_CHARS + 1),
    ] {
        let err = repo
            .insert_agent(&CreateAgent { name }, creator)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_agent_requires_active_user_creator() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let user = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_creator = insert_agent_account(&db.pool, user, "agent-creator").await?;

    let err = repo
        .insert_agent(
            &CreateAgent {
                name: "bot".to_owned(),
            },
            agent_creator,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::NotFound(message) if message == "agent creator user account not found")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn count_and_list_active_agents_per_creator() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let other = insert_user_account(&db.pool, "other", "o@example.test").await?;

    let a1 = make_agent(&repo, creator, "a1").await;
    let _a2 = make_agent(&repo, creator, "a2").await;
    let _b1 = make_agent(&repo, other, "b1").await;

    assert_eq!(repo.count_agents_by_creator(creator).await?, 2);
    assert_eq!(repo.count_agents_by_creator(other).await?, 1);
    assert_eq!(repo.list_agents_by_creator(creator).await?.len(), 2);

    repo.delete_agent(a1, creator).await?;
    assert_eq!(repo.count_agents_by_creator(creator).await?, 1);
    assert_eq!(repo.list_agents_by_creator(creator).await?.len(), 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn delete_agent_deactivates_account_and_revokes_keys_and_access()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let api_keys = ApiKeyRepo::new(db.pool.clone());
    let owner = insert_user_account(&db.pool, "owner", "owner@example.test").await?;
    let agent_id = make_agent(&repo, owner, "bot").await;
    let key_id = Uuid::new_v4();
    api_keys
        .insert_key_unchecked_for_test(InsertApiKey {
            key_id,
            account_id: agent_id,
            command: &CreateApiKey {
                name: "local-mcp".to_owned(),
                scopes: Vec::new(),
                expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
            },
            token_prefix: "ngk_v1_agent",
            token_hash: "hash-delete-token",
            created_by: owner,
            rotated_from_key_id: None,
        })
        .await?;
    let workspace_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (created_by, name) \
         VALUES ($1, 'personal') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO workspace_access (workspace_id, account_id, role, granted_by) \
         VALUES ($1, $2, 'editor', $3)",
    )
    .bind(workspace_id)
    .bind(agent_id)
    .bind(owner)
    .execute(&db.pool)
    .await?;

    repo.delete_agent(agent_id, owner).await?;

    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM accounts WHERE id = $1")
        .bind(agent_id)
        .fetch_one(&db.pool)
        .await?;
    assert!(!is_active);

    let key_revoked: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM api_keys WHERE id = $1")
            .bind(key_id)
            .fetch_one(&db.pool)
            .await?;
    assert!(key_revoked.is_some());

    let access_revoked: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT revoked_at FROM workspace_access WHERE workspace_id = $1 AND account_id = $2",
    )
    .bind(workspace_id)
    .bind(agent_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(access_revoked.is_some());

    db.cleanup().await;
    Ok(())
}
