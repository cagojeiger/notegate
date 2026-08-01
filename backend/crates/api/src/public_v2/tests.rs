#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::Router;
use axum::extract::Extension;
use axum::http::StatusCode;
use notegate_db::{AccountRepo, AgentRepo, ConnectionRepo, SpaceRepo, test_support::TestDb};
use notegate_model::{Caller, CallerIdentity, Channel, ConnectAgent, Permission, ResolveAttrs};
use notegate_service::agents::CreateAgent;
use notegate_service::spaces::CreateSpace;
use serde_json::json;

use crate::rest::test_support::{caller_and_space, empty_request, get_json, json_request, state};

fn app(state: crate::state::AppState, caller: Caller) -> Router {
    Router::new()
        .merge(super::routes())
        .layer(Extension(caller))
        .with_state(state)
}

async fn agent_caller(
    state: &crate::state::AppState,
    owner_id: uuid::Uuid,
    space_id: uuid::Uuid,
    permission: Permission,
) -> Result<Caller, Box<dyn std::error::Error>> {
    let agent = AgentRepo::new(state.db.clone())
        .insert_agent(
            &CreateAgent {
                name: format!("v2-{}-agent", permission.as_str()),
            },
            owner_id,
        )
        .await?;
    ConnectionRepo::new(state.db.clone())
        .upsert_connection(
            &ConnectAgent {
                space_id,
                agent_id: agent.id,
                permission,
            },
            owner_id,
        )
        .await?;
    let account = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .find_account(agent.id)
    .await?
    .expect("agent account");
    Ok(Caller {
        account,
        identity: CallerIdentity::Agent(agent),
        channel: Channel::Api,
    })
}

#[tokio::test]
async fn connected_write_agent_can_use_v2_resource_flow() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Write).await?;

    let (status, spaces) = get_json(app(state.clone(), caller.clone()), "/spaces".into()).await?;
    assert_eq!(status, StatusCode::OK, "{spaces}");
    assert_eq!(spaces["spaces"].as_array().map(Vec::len), Some(1));

    let (status, created) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "name": "agent-note.md",
            "kind": "text",
            "content": "first line\nneedle\n",
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let node_id = created["id"].as_str().expect("created node id");

    let (status, read) = get_json(
        app(state.clone(), caller.clone()),
        format!("/spaces/{space_id}/text/{node_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["text"]["content"], "first line\nneedle\n");

    let (status, found) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/search/grep"),
        json!({"q": "needle", "lines": "all"}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["items"][0]["id"], node_id);
    assert_eq!(found["items"][0]["match_lines"], json!([2]));

    let (status, deleted) = empty_request(
        app(state, caller),
        "DELETE",
        format!("/spaces/{space_id}/nodes/{node_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{deleted}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn v2_respects_connection_permission_and_space_visibility()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let caller = agent_caller(&state, owner.account_id(), space_id, Permission::Read).await?;

    let (status, denied) = json_request(
        app(state.clone(), caller.clone()),
        "POST",
        format!("/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "name": "denied.md",
            "kind": "text",
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (hidden_owner, _) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "v2-hidden-owner".to_owned(),
        email: "v2-hidden-owner@example.test".to_owned(),
        name: "V2 Hidden Owner".to_owned(),
    })
    .await?;
    let hidden = SpaceRepo::new(state.db.clone())
        .create_space(
            hidden_owner.id,
            &CreateSpace {
                name: "v2-hidden".to_owned(),
            },
        )
        .await?;
    let (status, hidden_response) =
        get_json(app(state, caller), format!("/spaces/{}", hidden.id)).await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden_response}");

    db.cleanup().await;
    Ok(())
}
