//! Integration tests for durable audit event capture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account, set_user_tier};
use notegate_db::api_key_repo::InsertApiKey;
use notegate_db::{AccountRepo, AgentRepo, ApiKeyRepo, ConnectionRepo, SpaceRepo};
use notegate_model::{ConnectAgent, CreateAgent, CreateApiKey, CreateSpace, Permission};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct AuditRow {
    op_type: String,
    owner_user_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    resource_type: String,
    resource_id: Option<Uuid>,
    metadata: Value,
}

async fn audit_rows(pool: &sqlx::PgPool) -> Result<Vec<AuditRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT op_type, owner_user_id, actor_account_id, resource_type, resource_id, metadata \
         FROM audit_events ORDER BY id",
    )
    .fetch_all(pool)
    .await
}

async fn insert_agent(
    repo: &AgentRepo,
    owner: Uuid,
    name: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(repo
        .insert_agent(
            &CreateAgent {
                name: name.to_owned(),
            },
            owner,
        )
        .await?
        .id)
}

#[tokio::test]
async fn space_mutations_write_audit_events() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "audit-space", "audit-space@example.test").await?;
    let repo = SpaceRepo::new(db.pool.clone());

    let space = repo
        .create_space(
            owner,
            &CreateSpace {
                name: "audit".to_owned(),
            },
        )
        .await?;
    repo.update_space(space.id, owner, Some("audit-renamed"), Some(2000))
        .await?;
    repo.delete_space(space.id, owner, owner).await?;

    let rows = audit_rows(&db.pool).await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].op_type, "space.create");
    assert_eq!(rows[1].op_type, "space.update");
    assert_eq!(rows[2].op_type, "space.delete");
    for row in &rows {
        assert_eq!(row.owner_user_id, Some(owner));
        assert_eq!(row.actor_account_id, Some(owner));
        assert_eq!(row.resource_type, "space");
        assert_eq!(row.resource_id, Some(space.id));
    }
    assert_eq!(
        rows[1].metadata["changed_fields"],
        serde_json::json!(["name", "sort_order"])
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn agent_connection_and_agent_key_mutations_write_audit_events()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "audit-agent", "audit-agent@example.test").await?;
    set_user_tier(&db.pool, owner, "system_max").await?;
    let agents = AgentRepo::new(db.pool.clone());
    let connections = ConnectionRepo::new(db.pool.clone());
    let api_keys = ApiKeyRepo::new(db.pool.clone());

    let agent = insert_agent(&agents, owner, "audit-bot").await?;
    let space_id: Uuid = sqlx::query_scalar(
        "INSERT INTO spaces (owner_user_id, name) VALUES ($1, 'audit-space') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    connections
        .upsert_connection(
            &ConnectAgent {
                space_id,
                agent_id: agent,
                permission: Permission::Write,
            },
            owner,
        )
        .await?;
    connections.disconnect(space_id, agent, owner).await?;

    let key_id = Uuid::new_v4();
    api_keys
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id: agent,
                command: &CreateApiKey {
                    name: "agent-key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                },
                token_prefix: "ngk_v1_agent",
                token_hash: "hash-agent-audit",
                created_by: owner,
                rotated_from_key_id: None,
            },
            notegate_core::limits::AGENT_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;
    agents.delete_agent(agent, owner).await?;

    let rows = audit_rows(&db.pool).await?;
    let op_types = rows
        .iter()
        .map(|row| row.op_type.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        op_types,
        vec![
            "agent.create",
            "connection.upsert",
            "connection.disconnect",
            "agent_key.create",
            "agent.delete",
        ]
    );

    let key_event = rows
        .iter()
        .find(|row| row.op_type == "agent_key.create")
        .expect("agent key audit event");
    assert_eq!(key_event.owner_user_id, Some(owner));
    assert_eq!(key_event.actor_account_id, Some(owner));
    assert_eq!(key_event.resource_type, "api_key");
    assert_eq!(key_event.resource_id, Some(key_id));

    let delete_event = rows
        .iter()
        .find(|row| row.op_type == "agent.delete")
        .expect("agent delete audit event");
    assert_eq!(
        delete_event.metadata["revoked_agent_keys"],
        serde_json::json!(1)
    );
    assert_eq!(
        delete_event.metadata["disconnected_connections"],
        serde_json::json!(0)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn user_key_and_account_delete_mutations_write_audit_events()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let user = insert_user_account(&db.pool, "audit-user", "audit-user@example.test").await?;
    let api_keys = ApiKeyRepo::new(db.pool.clone());

    let first_key = Uuid::new_v4();
    api_keys
        .insert_key_with_cap(
            InsertApiKey {
                key_id: first_key,
                account_id: user,
                command: &CreateApiKey {
                    name: "user-key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                },
                token_prefix: "ngk_v1_user",
                token_hash: "hash-user-audit-1",
                created_by: user,
                rotated_from_key_id: None,
            },
            notegate_core::limits::USER_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;
    let rotated_key = Uuid::new_v4();
    api_keys
        .rotate_key(
            InsertApiKey {
                key_id: rotated_key,
                account_id: user,
                command: &CreateApiKey {
                    name: "user-key".to_owned(),
                    scopes: Vec::new(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                },
                token_prefix: "ngk_v1_user",
                token_hash: "hash-user-audit-2",
                created_by: user,
                rotated_from_key_id: Some(first_key),
            },
            first_key,
            user,
            notegate_core::limits::USER_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;
    api_keys
        .revoke_key(user, rotated_key, user, Some("manual"))
        .await?;
    AccountRepo::new(db.pool.clone())
        .soft_delete_user(user, user)
        .await?;

    let rows = audit_rows(&db.pool).await?;
    let op_types = rows
        .iter()
        .map(|row| row.op_type.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        op_types,
        vec![
            "user_key.create",
            "user_key.rotate",
            "user_key.revoke",
            "account.delete",
        ]
    );
    assert_eq!(
        rows[1].metadata["rotated_from_key_id"],
        serde_json::json!(first_key)
    );
    assert_eq!(rows[2].metadata["reason"], serde_json::json!("manual"));
    assert_eq!(rows[3].metadata["revoked_api_keys"], serde_json::json!(0));

    db.cleanup().await;
    Ok(())
}
