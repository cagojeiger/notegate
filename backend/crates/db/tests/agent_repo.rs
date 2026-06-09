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
use notegate_db::AgentRepo;
use notegate_model::{CreateAgent, CreateAgentKey};
use uuid::Uuid;

fn fake_token_hash(token: &str) -> String {
    format!("hash-{token}")
}

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
async fn create_key_stores_hash_only() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "super-secret-token";
    let token_hash = fake_token_hash(plaintext);
    let key = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "local-mcp".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &token_hash,
            creator,
        )
        .await?;
    assert_eq!(key.token_hash, token_hash);

    // The plaintext is never persisted; only the hash is stored.
    let stored: String = sqlx::query_scalar("SELECT token_hash FROM api_keys WHERE id = $1")
        .bind(key.id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(stored, token_hash);
    assert_ne!(stored, plaintext);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_key_rejects_non_empty_scopes() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let err = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "scoped-key".to_owned(),
                scopes: vec!["files:read".to_owned()],
                expires_at: None,
            },
            &fake_token_hash("scoped-token"),
            creator,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Validation(message) if message == "agent key scopes must be empty")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_key_requires_owned_active_agent() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let other = insert_user_account(&db.pool, "other", "o@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let err = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "wrong-owner-key".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("wrong-owner-token"),
            other,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(message) if message == "agent not found"));

    repo.delete_agent(agent_id, creator).await?;
    let err = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "inactive-key".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("inactive-token"),
            creator,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(message) if message == "agent not found"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn auth_accepts_valid_key() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "valid-token";
    repo.insert_agent_key(
        &CreateAgentKey {
            agent_id,
            name: "k".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        },
        &fake_token_hash(plaintext),
        creator,
    )
    .await?;

    let (account, agent) = repo
        .find_agent_by_key_hash(&fake_token_hash(plaintext))
        .await?
        .ok_or("valid key authenticates")?;
    assert_eq!(account.id, agent_id);
    assert_eq!(account.kind.as_str(), "agent");
    assert_eq!(agent.id, agent_id);

    // last_used_at is recorded on successful auth.
    let last_used: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE token_hash = $1")
            .bind(fake_token_hash(plaintext))
            .fetch_one(&db.pool)
            .await?;
    assert!(last_used.is_some());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn auth_rejects_revoked_key() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "revoked-token";
    let key = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "k".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash(plaintext),
            creator,
        )
        .await?;
    repo.revoke_key(agent_id, key.id, creator).await?;

    let resolved = repo
        .find_agent_by_key_hash(&fake_token_hash(plaintext))
        .await?;
    assert!(resolved.is_none(), "revoked key must not authenticate");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn auth_rejects_expired_key() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "expired-token";
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    repo.insert_agent_key(
        &CreateAgentKey {
            agent_id,
            name: "k".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(past),
        },
        &fake_token_hash(plaintext),
        creator,
    )
    .await?;

    let resolved = repo
        .find_agent_by_key_hash(&fake_token_hash(plaintext))
        .await?;
    assert!(resolved.is_none(), "expired key must not authenticate");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn auth_rejects_inactive_agent_account() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "live-token";
    repo.insert_agent_key(
        &CreateAgentKey {
            agent_id,
            name: "k".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        },
        &fake_token_hash(plaintext),
        creator,
    )
    .await?;

    // Deactivate the agent account; the (still non-revoked) key must stop working.
    repo.delete_agent(agent_id, creator).await?;

    let resolved = repo
        .find_agent_by_key_hash(&fake_token_hash(plaintext))
        .await?;
    assert!(resolved.is_none(), "inactive agent must not authenticate");

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

    // Deleting one agent drops it from the active count/list.
    repo.delete_agent(a1, creator).await?;
    assert_eq!(repo.count_agents_by_creator(creator).await?, 1);
    assert_eq!(repo.list_agents_by_creator(creator).await?.len(), 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn count_live_keys_excludes_revoked_and_expired() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let k1 = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "k1".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("t1"),
            creator,
        )
        .await?;
    repo.insert_agent_key(
        &CreateAgentKey {
            agent_id,
            name: "k2".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        },
        &fake_token_hash("t2"),
        creator,
    )
    .await?;
    repo.insert_agent_key(
        &CreateAgentKey {
            agent_id,
            name: "expired".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
        },
        &fake_token_hash("expired"),
        creator,
    )
    .await?;
    assert_eq!(repo.count_live_keys(agent_id).await?, 2);

    repo.revoke_key(agent_id, k1.id, creator).await?;
    assert_eq!(repo.count_live_keys(agent_id).await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn create_key_enforces_live_key_cap_in_repo() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    for index in 0..limits::AGENT_KEYS_PER_AGENT_MAX {
        repo.insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: format!("key-{index}"),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash(&format!("token-{index}")),
            creator,
        )
        .await?;
    }

    let err = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "overflow".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("overflow-token"),
            creator,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Conflict(_)));

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
    let owner = insert_user_account(&db.pool, "owner", "owner@example.test").await?;
    let agent_id = make_agent(&repo, owner, "bot").await;
    let key = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id,
                name: "local-mcp".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("delete-token"),
            owner,
        )
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
            .bind(key.id)
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

#[tokio::test]
async fn revoke_key_is_scoped_to_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_a = make_agent(&repo, creator, "a").await;
    let agent_b = make_agent(&repo, creator, "b").await;

    let key_b = repo
        .insert_agent_key(
            &CreateAgentKey {
                agent_id: agent_b,
                name: "b-key".to_owned(),
                scopes: Vec::new(),
                expires_at: None,
            },
            &fake_token_hash("b-token"),
            creator,
        )
        .await?;

    let result = repo.revoke_key(agent_a, key_b.id, creator).await;
    assert!(
        result.is_err(),
        "a key cannot be revoked through another agent id"
    );

    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM api_keys WHERE id = $1")
            .bind(key_b.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(
        revoked_at.is_none(),
        "cross-agent revoke must not change the key"
    );

    repo.revoke_key(agent_b, key_b.id, creator).await?;
    assert_eq!(repo.count_live_keys(agent_b).await?, 0);

    db.cleanup().await;
    Ok(())
}
