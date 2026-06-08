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
use notegate_db::AgentRepo;
use notegate_service::agents::{AgentStore, CreateAgent, CreateAgentKey, hash_token};
use notegate_service::identity::AgentAuthStore;
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
async fn create_key_stores_hash_only() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let repo = AgentRepo::new(db.pool.clone());
    let creator = insert_user_account(&db.pool, "creator", "c@example.test").await?;
    let agent_id = make_agent(&repo, creator, "bot").await;

    let plaintext = "super-secret-token";
    let token_hash = hash_token(plaintext);
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
    let stored: String = sqlx::query_scalar("SELECT token_hash FROM agent_keys WHERE id = $1")
        .bind(key.id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(stored, token_hash);
    assert_ne!(stored, plaintext);

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
        &hash_token(plaintext),
        creator,
    )
    .await?;

    let (account, agent) = repo
        .find_agent_by_key_hash(&hash_token(plaintext))
        .await?
        .ok_or("valid key authenticates")?;
    assert_eq!(account.id, agent_id);
    assert_eq!(account.kind.as_str(), "agent");
    assert_eq!(agent.id, agent_id);

    // last_used_at is recorded on successful auth.
    let last_used: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM agent_keys WHERE token_hash = $1")
            .bind(hash_token(plaintext))
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
            &hash_token(plaintext),
            creator,
        )
        .await?;
    repo.revoke_key(agent_id, key.id, creator).await?;

    let resolved = repo.find_agent_by_key_hash(&hash_token(plaintext)).await?;
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
        &hash_token(plaintext),
        creator,
    )
    .await?;

    let resolved = repo.find_agent_by_key_hash(&hash_token(plaintext)).await?;
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
        &hash_token(plaintext),
        creator,
    )
    .await?;

    // Deactivate the agent account; the (still non-revoked) key must stop working.
    repo.delete_agent(agent_id, creator).await?;

    let resolved = repo.find_agent_by_key_hash(&hash_token(plaintext)).await?;
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
async fn count_active_keys_excludes_revoked() -> Result<(), Box<dyn std::error::Error>> {
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
            &hash_token("t1"),
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
        &hash_token("t2"),
        creator,
    )
    .await?;
    assert_eq!(repo.count_active_keys(agent_id).await?, 2);

    repo.revoke_key(agent_id, k1.id, creator).await?;
    assert_eq!(repo.count_active_keys(agent_id).await?, 1);

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
            &hash_token("b-token"),
            creator,
        )
        .await?;

    let result = repo.revoke_key(agent_a, key_b.id, creator).await;
    assert!(
        result.is_err(),
        "a key cannot be revoked through another agent id"
    );

    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM agent_keys WHERE id = $1")
            .bind(key_b.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(
        revoked_at.is_none(),
        "cross-agent revoke must not change the key"
    );

    repo.revoke_key(agent_b, key_b.id, creator).await?;
    assert_eq!(repo.count_active_keys(agent_b).await?, 0);

    db.cleanup().await;
    Ok(())
}
