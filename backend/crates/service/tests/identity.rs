#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::{Duration, Utc};
use common::{TestDb, insert_user_account};
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, AgentRepo, ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::{Channel, CreateAgent, CreateApiKey};
use notegate_service::identity::{IdentityError, Resolver};
use uuid::Uuid;

async fn insert_key(
    repo: &ApiKeyRepo,
    crypto: &PiiCrypto,
    account_id: Uuid,
    created_by: Uuid,
    format_prefix: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let key_id = Uuid::new_v4();
    let secret = "identity-test-secret";
    let token_hash = crypto.api_key_hash(&key_id.to_string(), secret)?;
    let prefix = format!("{format_prefix}{key_id}");
    let command = CreateApiKey {
        name: "identity-test".to_owned(),
        scopes: Vec::new(),
        expires_at: Some(Utc::now() + Duration::days(1)),
    };
    repo.insert_key_unchecked_for_test(InsertApiKey {
        key_id,
        account_id,
        command: &command,
        token_prefix: &prefix,
        token_hash: &token_hash,
        created_by,
        rotated_from_key_id: None,
    })
    .await?;
    Ok(format!("{prefix}_{secret}"))
}

fn resolver(db: &TestDb, crypto: PiiCrypto) -> Resolver {
    Resolver::new(
        AccountRepo::with_crypto(db.pool.clone(), crypto.clone()),
        AgentRepo::new(db.pool.clone()),
        ApiKeyRepo::with_lookup_key(db.pool.clone(), crypto.lookup_key_id(), crypto.version()),
        crypto,
    )
}

#[tokio::test]
async fn agent_api_key_resolution_rejects_historical_user_owned_key()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let crypto = PiiCrypto::test();
    let user_id = insert_user_account(&db.pool, "identity-user", "identity@example.test").await?;
    let key_repo =
        ApiKeyRepo::with_lookup_key(db.pool.clone(), crypto.lookup_key_id(), crypto.version());
    sqlx::query("ALTER TABLE api_keys DISABLE TRIGGER api_keys_v2_agent_owner")
        .execute(&db.pool)
        .await?;
    let token = insert_key(&key_repo, &crypto, user_id, user_id, "ngk_v2_").await?;
    sqlx::query("ALTER TABLE api_keys ENABLE TRIGGER api_keys_v2_agent_owner")
        .execute(&db.pool)
        .await?;

    let result = resolver(&db, crypto)
        .resolve_agent_api_key(&token, Channel::Api)
        .await;

    assert!(matches!(result, Err(IdentityError::NotRegistered)));
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_api_key_resolution_returns_agent_caller() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let crypto = PiiCrypto::test();
    let user_id = insert_user_account(
        &db.pool,
        "identity-agent-owner",
        "identity-owner@example.test",
    )
    .await?;
    let agent = AgentRepo::new(db.pool.clone())
        .insert_agent(
            &CreateAgent {
                name: "identity-agent".to_owned(),
            },
            user_id,
        )
        .await?;
    let key_repo =
        ApiKeyRepo::with_lookup_key(db.pool.clone(), crypto.lookup_key_id(), crypto.version());
    let resolver = resolver(&db, crypto.clone());
    let token = insert_key(&key_repo, &crypto, agent.id, user_id, "ngk_v2_").await?;
    let caller = resolver.resolve_agent_api_key(&token, Channel::Mcp).await?;

    assert_eq!(caller.account_id(), agent.id);
    assert_eq!(caller.channel, Channel::Mcp);
    assert_eq!(
        caller.agent().map(|agent| agent.name.as_str()),
        Some("identity-agent")
    );

    let legacy = format!("ngk_v1_{}_legacy-secret", Uuid::new_v4());
    assert!(matches!(
        resolver.resolve_agent_api_key(&legacy, Channel::Mcp).await,
        Err(IdentityError::NotRegistered)
    ));
    db.cleanup().await;
    Ok(())
}
