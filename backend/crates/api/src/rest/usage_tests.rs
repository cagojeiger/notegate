#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::{StatusCode, header::RETRY_AFTER};
use notegate_core::tier::effective_file_tree_limits;
use notegate_db::{AgentRepo, test_support::TestDb};
use notegate_model::{Caller, CallerIdentity, Channel, CreateAgent, ResolveAttrs};
use serde_json::json;
use uuid::Uuid;

use super::test_support::{
    caller_and_space, decode_response, empty_request, get_json, json_response, rest_app, state,
};

const SPACE_GATE_NAMESPACE: u64 = 0x4e47_5350_4143_4501;

fn space_gate_seed(space_id: Uuid) -> i64 {
    let value = space_id.as_u128();
    let folded = (value as u64) ^ ((value >> 64) as u64) ^ SPACE_GATE_NAMESPACE;
    i64::from_ne_bytes(folded.to_ne_bytes())
}

#[tokio::test]
async fn rest_usage_endpoints_enforce_the_public_contract() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let owner_id = owner.account_id();
    let limits = effective_file_tree_limits(state.config.default_user_tier, state.config.limits);

    let (status, usage) = get_json(
        rest_app(state.clone(), owner.clone()),
        "/v1/me/usage".into(),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{usage}");
    assert_eq!(
        usage["tier"],
        json!(state.config.default_user_tier.as_str())
    );
    assert!(usage.get("account").is_none());
    assert_eq!(usage["spaces"][0]["id"], json!(space_id));
    assert_eq!(usage["spaces"][0]["items"]["used"], json!(0));
    assert_eq!(
        usage["spaces"][0]["items"]["limit"],
        json!(limits.space_max_nodes.saturating_sub(1))
    );
    assert_eq!(usage["spaces"][0]["text_bytes"]["used"], json!(0));
    assert_eq!(
        usage["spaces"][0]["text_bytes"]["limit"],
        json!(limits.space_max_text_bytes)
    );
    assert_eq!(usage["spaces"][0]["file_bytes"]["used"], json!(0));
    assert_eq!(
        usage["spaces"][0]["file_bytes"]["limit"],
        json!(limits.space_max_file_bytes)
    );
    assert!(usage["spaces"][0].get("content_bytes").is_none());
    assert!(usage["spaces"][0].get("agent_connections").is_none());
    assert_eq!(usage["spaces"][0]["reconciliation_pending"], json!(false));

    let (status, cooldown) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/usage/reconcile"),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{cooldown}");
    assert_eq!(cooldown["kind"], json!("usage_reconciliation_cooldown"));

    sqlx::query(
        "UPDATE space_usage \
         SET reconciled_at = now() - interval '2 hours' \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;
    let (status, queued) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/usage/reconcile"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["status"], json!("queued"));
    let job_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM space_usage_reconcile_jobs WHERE space_id = $1)",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(job_exists);

    let (status, duplicate) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/usage/reconcile"),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT, "{duplicate}");
    assert_eq!(duplicate["kind"], json!("usage_reconciliation_pending"));

    let (stranger_account, stranger_user) = state
        .accounts
        .upsert_user_by_sub(&ResolveAttrs {
            sub: "rest-usage-stranger".to_owned(),
            email: "rest-usage-stranger@example.test".to_owned(),
            name: "REST Usage Stranger".to_owned(),
        })
        .await?;
    let stranger = Caller {
        account: stranger_account,
        identity: CallerIdentity::User(stranger_user),
        channel: Channel::Browser,
    };
    let (status, hidden) = empty_request(
        rest_app(state.clone(), stranger),
        "POST",
        format!("/v1/spaces/{space_id}/usage/reconcile"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");
    assert_eq!(hidden["kind"], json!("not_found"));

    let agent = AgentRepo::new(state.db.clone())
        .insert_agent(
            &CreateAgent {
                name: "rest-usage-agent".to_owned(),
            },
            owner_id,
        )
        .await?;
    let agent_account = state
        .accounts
        .find_account(agent.id)
        .await?
        .expect("agent account");
    let agent_caller = Caller {
        account: agent_account,
        identity: CallerIdentity::Agent(agent),
        channel: Channel::Api,
    };
    let (status, forbidden) = get_json(
        rest_app(state.clone(), agent_caller.clone()),
        "/v1/me/usage".into(),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "{forbidden}");
    let (status, forbidden) = empty_request(
        rest_app(state.clone(), agent_caller),
        "POST",
        format!("/v1/spaces/{space_id}/usage/reconcile"),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "{forbidden}");

    let mut maintenance_tx = db.pool.begin().await?;
    let gate_acquired: bool = sqlx::query_scalar(
        "SELECT pg_try_advisory_xact_lock(hashtextextended(current_schema(), $1))",
    )
    .bind(space_gate_seed(space_id))
    .fetch_one(&mut *maintenance_tx)
    .await?;
    assert!(gate_acquired);
    let response = json_response(
        rest_app(state.clone(), owner),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({"parent_id": root_id, "kind": "folder", "name": "blocked"}),
    )
    .await?;
    assert_eq!(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("5")
    );
    let (status, maintenance) = decode_response(response).await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{maintenance}");
    assert_eq!(
        maintenance["kind"],
        json!("usage_recalculation_in_progress")
    );
    maintenance_tx.commit().await?;

    db.cleanup().await;
    Ok(())
}
