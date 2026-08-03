//! Integration coverage for current-user event history endpoints.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_in_result
)]

use axum::http::{StatusCode, header::CACHE_CONTROL};
use notegate_db::{NewMcpInvocation, test_support::TestDb};

use super::test_support::{
    caller_and_space, decode_response, get_json, json_response, rest_app, state,
};

#[tokio::test]
async fn mcp_invocations_returns_only_the_current_users_paginated_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, _, _) = caller_and_space(&state).await?;
    let owner = caller.account_id();
    let input = serde_json::json!({"op": "changes", "target": "Research:/"});

    for purpose in ["first purpose", "second purpose"] {
        state
            .mcp_invocations
            .insert(NewMcpInvocation {
                owner_user_id: owner,
                actor_account_id: owner,
                caller_kind: "user",
                tool: "read",
                op: Some("changes"),
                purpose: Some(purpose),
                space_name: Some("Research"),
                input: &input,
                outcome: "success",
                error_code: None,
                duration_ms: 4,
            })
            .await?;
    }

    let app = rest_app(state.clone(), caller.clone());
    let response = json_response(
        app,
        "GET",
        "/v1/me/mcp-invocations?limit=1".to_owned(),
        serde_json::json!({}),
    )
    .await?;
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );
    let (status, first) = decode_response(response).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["invocations"][0]["purpose"], "second purpose");
    assert_eq!(first["invocations"][0]["space_name"], "Research");
    assert_eq!(first["invocations"][0]["input"], input);
    assert_eq!(
        first["invocations"][0]["actor"]["display_name"],
        "REST Test Owner"
    );
    assert_eq!(first["page"]["returned"], 1);
    assert_eq!(first["page"]["has_more"], true);

    let cursor = first["page"]["next_cursor"].as_str().expect("next cursor");
    let app = rest_app(state, caller);
    let (status, second) = get_json(
        app,
        format!("/v1/me/mcp-invocations?limit=1&cursor={cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["invocations"][0]["purpose"], "first purpose");
    assert_eq!(second["page"]["has_more"], false);

    db.cleanup().await;
    Ok(())
}
