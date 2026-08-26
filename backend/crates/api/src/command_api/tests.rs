#![allow(clippy::indexing_slicing)]

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Extension;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Request, StatusCode};
use notegate_db::test_support::TestDb;
use notegate_model::Channel;
use notegate_service::spaces::UpdateSpace;
use serde_json::Value;
use serde_json::json;
use tower::ServiceExt as _;

use super::*;
use crate::rest::test_support::{caller_and_space, state};

fn cli_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-notegate-cli-version",
        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    headers.insert(
        "x-notegate-command-protocol",
        HeaderValue::from_static(notegate_command::COMMAND_PROTOCOL_VERSION),
    );
    headers
}

async fn cli_request(
    app: Router,
    headers: HeaderMap,
    body: Value,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/cli")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))?;
    request.headers_mut().extend(headers);
    let response = app.oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[tokio::test]
async fn cli_read_uses_the_shared_engine_and_records_the_cli_surface()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (mut caller, space_id, _root_id) = caller_and_space(&state).await?;
    state
        .spaces
        .update(
            caller.account.kind,
            caller.account_id(),
            UpdateSpace {
                space_id,
                name: None,
                sort_order: None,
                navigation_pinned: None,
                user_mcp_enabled: Some(true),
                default_search_enabled: None,
                default_text_encryption_enabled: None,
            },
        )
        .await?;
    caller.channel = Channel::Api;
    let app = Router::new()
        .merge(routes())
        .layer(Extension(caller.clone()))
        .with_state(state.clone());

    let (status, spaces) = cli_request(
        app.clone(),
        cli_headers(),
        json!({
            "tool": "read",
            "input": {
                "purpose": "list accessible spaces",
                "op": "spaces"
            }
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{spaces}");
    assert_eq!(spaces["spaces"][0]["name"], "rest-test");

    let (status, error) = cli_request(
        app,
        cli_headers(),
        json!({
            "tool": "read",
            "input": {
                "purpose": "read a text node",
                "op": "read"
            }
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["error"], "required_field_missing");
    assert_eq!(error["data"]["next_action"]["kind"], "add_fields");

    let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
        "SELECT surface, tool, outcome, error_code FROM command_invocations \
         WHERE actor_account_id = $1 ORDER BY id",
    )
    .bind(caller.account_id())
    .fetch_all(&state.db)
    .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        (
            "cli".to_owned(),
            "read".to_owned(),
            "success".to_owned(),
            None
        )
    );
    assert_eq!(rows[1].0, "cli");
    assert_eq!(rows[1].2, "error");
    assert_eq!(rows[1].3.as_deref(), Some("required_field_missing"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rejected_cli_me_input_never_records_a_purpose() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (mut caller, _space_id, _root_id) = caller_and_space(&state).await?;
    caller.channel = Channel::Api;
    let app = Router::new()
        .merge(routes())
        .layer(Extension(caller.clone()))
        .with_state(state.clone());

    let (status, body) = cli_request(
        app,
        cli_headers(),
        json!({
            "tool": "me",
            "input": {"purpose": "must not be persisted"}
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "me_input_must_be_empty");
    let purpose = sqlx::query_scalar::<_, Option<String>>(
        "SELECT purpose FROM command_invocations \
         WHERE actor_account_id = $1 AND surface = 'cli' AND tool = 'me'",
    )
    .bind(caller.account_id())
    .fetch_one(&state.db)
    .await?;
    assert_eq!(purpose, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn compatible_protocol_accepts_a_different_cli_release()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (mut caller, _space_id, _root_id) = caller_and_space(&state).await?;
    caller.channel = Channel::Api;
    let app = Router::new()
        .merge(routes())
        .layer(Extension(caller))
        .with_state(state);
    let mut headers = HeaderMap::new();
    headers.insert("x-notegate-cli-version", HeaderValue::from_static("0.0.0"));
    headers.insert(
        "x-notegate-command-protocol",
        HeaderValue::from_static(notegate_command::COMMAND_PROTOCOL_VERSION),
    );

    let (status, body) = cli_request(app, headers, json!({"tool":"me","input":{}})).await?;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["server_version"], env!("CARGO_PKG_VERSION"));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn missing_or_incompatible_cli_protocol_receives_one_structured_update_action()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (mut caller, _space_id, _root_id) = caller_and_space(&state).await?;
    caller.channel = Channel::Api;
    let app = Router::new()
        .merge(routes())
        .layer(Extension(caller))
        .with_state(state);
    let mut missing_protocol_headers = HeaderMap::new();
    missing_protocol_headers.insert(
        "x-notegate-cli-version",
        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );

    let (status, body) = cli_request(
        app.clone(),
        missing_protocol_headers,
        json!({"tool":"me","input":{}}),
    )
    .await?;

    assert_eq!(status, StatusCode::UPGRADE_REQUIRED, "{body}");
    assert_eq!(body["error"], "cli_update_required");
    assert_eq!(body["data"]["client_protocol_version"], Value::Null);

    let mut headers = cli_headers();
    headers.insert(
        "x-notegate-command-protocol",
        HeaderValue::from_static("unsupported"),
    );

    let (status, body) = cli_request(app, headers, json!({"tool":"me","input":{}})).await?;

    assert_eq!(status, StatusCode::UPGRADE_REQUIRED, "{body}");
    assert_eq!(body["error"], "cli_update_required");
    assert_eq!(body["kind"], "client_protocol_incompatible");
    assert_eq!(body["data"]["client_protocol_version"], "unsupported");
    assert_eq!(
        body["data"]["server_protocol_version"],
        notegate_command::COMMAND_PROTOCOL_VERSION
    );
    assert_eq!(body["data"]["next_action"]["kind"], "run_command");
    assert_eq!(
        body["data"]["next_action"]["command"],
        "notegate-cli update"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cli_and_mcp_sequence_inputs_share_the_same_common_type() {
    let read = json!({
        "purpose": "inspect related notes",
        "commands": [
            {"tool": "read", "op": "spaces"},
            {"tool": "search", "op": "find", "target": "daily:/", "q": "notes"}
        ]
    });
    assert!(serde_json::from_value::<notegate_command::RunReadSequenceInput>(read).is_ok());

    let write = json!({
        "purpose": "create a folder",
        "commands": [
            {"tool": "manage", "op": "mkdir", "target": "daily:/notes", "parents": true}
        ]
    });
    assert!(serde_json::from_value::<notegate_command::RunWriteSequenceInput>(write).is_ok());
}

#[tokio::test]
async fn cli_read_sequence_executes_the_shared_engine_and_records_one_invocation()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (mut caller, space_id, _root_id) = caller_and_space(&state).await?;
    state
        .spaces
        .update(
            caller.account.kind,
            caller.account_id(),
            UpdateSpace {
                space_id,
                name: None,
                sort_order: None,
                navigation_pinned: None,
                user_mcp_enabled: Some(true),
                default_search_enabled: None,
                default_text_encryption_enabled: None,
            },
        )
        .await?;
    caller.channel = Channel::Api;
    let app = Router::new()
        .merge(routes())
        .layer(Extension(caller.clone()))
        .with_state(state.clone());

    let (status, body) = cli_request(
        app,
        cli_headers(),
        json!({
            "tool": "run_read_sequence",
            "input": {
                "purpose": "inspect spaces in parallel",
                "commands": [
                    {"tool": "read", "op": "spaces"},
                    {"tool": "read", "op": "spaces", "limit": 1}
                ]
            }
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["completed"], 2);
    assert_eq!(body["results"][0]["index"], 0);
    assert_eq!(body["results"][1]["index"], 1);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM command_invocations \
         WHERE actor_account_id = $1 AND surface = 'cli' AND tool = 'run_read_sequence'",
    )
    .bind(caller.account_id())
    .fetch_one(&state.db)
    .await?;
    assert_eq!(count, 1);

    db.cleanup().await;
    Ok(())
}
