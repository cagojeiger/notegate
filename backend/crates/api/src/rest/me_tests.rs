//! Integration coverage for current-user event history endpoints.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_in_result
)]

use axum::http::{StatusCode, header::CACHE_CONTROL};
use notegate_db::{NewCommandInvocation, test_support::TestDb};
use uuid::Uuid;

use super::test_support::{
    caller_and_space, decode_response, get_json, json_response, rest_app, state,
};

#[tokio::test]
async fn command_invocations_require_one_surface_and_keep_pagination_independent()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, _, _) = caller_and_space(&state).await?;
    let owner = caller.account_id();
    let input = serde_json::json!({"op": "changes", "target": "Research:/"});
    let recorded_response = serde_json::json!({
        "kind": "complete",
        "is_error": false,
        "result": {"space": "Research", "events": []}
    });

    for (surface, purpose, response) in [
        ("mcp", "older MCP purpose", None),
        ("cli", "older CLI purpose", None),
        ("mcp", "newer MCP purpose", None),
        ("cli", "newer CLI purpose", Some(&recorded_response)),
    ] {
        state
            .command_invocations
            .insert(NewCommandInvocation {
                owner_user_id: owner,
                actor_account_id: owner,
                caller_kind: "user",
                surface,
                tool: "read",
                op: Some("changes"),
                purpose: Some(purpose),
                space_name: Some("Research"),
                input: &input,
                response,
                outcome: "success",
                error_code: None,
                duration_ms: 4,
            })
            .await?;
    }

    let app = rest_app(state.clone(), caller.clone());
    let (status, _) = get_json(app, "/v1/me/command-invocations?limit=1".to_owned()).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let app = rest_app(state.clone(), caller.clone());
    let (status, _) = get_json(
        app,
        "/v1/me/command-invocations?surface=command_api&limit=1".to_owned(),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let app = rest_app(state.clone(), caller.clone());
    let response = json_response(
        app,
        "GET",
        "/v1/me/command-invocations?surface=mcp&limit=1".to_owned(),
        serde_json::json!({}),
    )
    .await?;
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );
    let (status, first) = decode_response(response).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        first["command_invocations"][0]["purpose"],
        "newer MCP purpose"
    );
    assert_eq!(first["command_invocations"][0]["surface"], "mcp");
    assert_eq!(first["command_invocations"][0]["space_name"], "Research");
    assert_eq!(first["command_invocations"][0]["input"], input);
    assert_eq!(
        first["command_invocations"][0]["response"],
        serde_json::Value::Null
    );
    assert_eq!(
        first["command_invocations"][0]["actor"]["display_name"],
        "REST Test Owner"
    );
    assert_eq!(first["page"]["returned"], 1);
    assert_eq!(first["page"]["has_more"], true);

    let mcp_cursor = first["page"]["next_cursor"]
        .as_str()
        .expect("MCP next cursor");

    let app = rest_app(state.clone(), caller.clone());
    let (status, _) = get_json(
        app,
        format!("/v1/me/command-invocations?surface=cli&limit=1&cursor={mcp_cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let app = rest_app(state.clone(), caller.clone());
    let (status, second) = get_json(
        app,
        format!("/v1/me/command-invocations?surface=mcp&limit=1&cursor={mcp_cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        second["command_invocations"][0]["purpose"],
        "older MCP purpose"
    );
    assert_eq!(second["command_invocations"][0]["surface"], "mcp");
    assert_eq!(
        second["command_invocations"][0]["response"],
        serde_json::Value::Null
    );
    assert_eq!(second["page"]["has_more"], false);

    let app = rest_app(state.clone(), caller.clone());
    let (status, cli_first) = get_json(
        app,
        "/v1/me/command-invocations?surface=cli&limit=1".to_owned(),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cli_first["command_invocations"][0]["purpose"],
        "newer CLI purpose"
    );
    assert_eq!(cli_first["command_invocations"][0]["surface"], "cli");
    assert_eq!(
        cli_first["command_invocations"][0]["response"],
        recorded_response
    );
    let cli_cursor = cli_first["page"]["next_cursor"]
        .as_str()
        .expect("CLI next cursor");

    let app = rest_app(state, caller);
    let (status, cli_second) = get_json(
        app,
        format!("/v1/me/command-invocations?surface=cli&limit=1&cursor={cli_cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cli_second["command_invocations"][0]["purpose"],
        "older CLI purpose"
    );
    assert_eq!(cli_second["command_invocations"][0]["surface"], "cli");
    assert_eq!(cli_second["page"]["has_more"], false);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn background_jobs_return_owned_queue_history_and_attempts()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space_id, _) = caller_and_space(&state).await?;
    let owner_account_id = caller.account_id();
    let job_id: Uuid = sqlx::query_scalar(
        "INSERT INTO background_jobs \
         (job_kind, payload, status, attempt_count, max_attempts, completed_at, \
          history_visibility, history_owner_account_id, context_kind, context_id, context_label) \
         VALUES ('space_usage_reconcile', jsonb_build_object('space_id', $1), \
                 'succeeded', 1, 8, now(), 'visible', $2, 'space', $1, 'rest-test') \
         RETURNING job_id",
    )
    .bind(space_id)
    .bind(owner_account_id)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO background_job_attempts \
         (job_id, attempt_number, claim_token, worker_id, started_at, finished_at, outcome) \
         VALUES ($1, 1, $2, 'private-worker-name', now(), now(), 'succeeded')",
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .execute(&db.pool)
    .await?;

    let app = rest_app(state.clone(), caller.clone());
    let response = json_response(
        app,
        "GET",
        "/v1/me/jobs?limit=10".to_owned(),
        serde_json::json!({}),
    )
    .await?;
    assert_eq!(
        response.headers().get(CACHE_CONTROL),
        Some(&"private, no-store".parse()?)
    );
    let (status, list) = decode_response(response).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["jobs"][0]["id"], job_id.to_string());
    assert_eq!(list["jobs"][0]["context_kind"], "space");
    assert_eq!(list["jobs"][0]["context_label"], "rest-test");
    assert_eq!(list["jobs"][0]["status"], "succeeded");

    let app = rest_app(state, caller);
    let (status, detail) = get_json(app, format!("/v1/me/jobs/{job_id}")).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["attempts"][0]["attempt_number"], 1);
    assert_eq!(detail["attempts"][0]["outcome"], "succeeded");
    assert!(detail["attempts"][0].get("worker_id").is_none());

    db.cleanup().await;
    Ok(())
}
