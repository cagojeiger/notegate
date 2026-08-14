#![allow(clippy::unwrap_used, clippy::expect_used, clippy::unwrap_in_result)]

use axum::http::StatusCode;
use notegate_db::test_support::TestDb;
use serde_json::json;
use uuid::Uuid;

use crate::rest::test_support::{caller_and_space, get_json, json_request, rest_app, state};

#[tokio::test]
async fn node_metadata_is_read_only_over_rest() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;

    let (status, node) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "kind": "text",
            "name": "metadata.md"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{node}");
    let node_id: Uuid = serde_json::from_value(
        node.get("id")
            .cloned()
            .ok_or_else(|| std::io::Error::other("created node response missing id"))?,
    )?;
    let uri = format!("/v1/spaces/{space_id}/nodes/{node_id}/metadata");

    let (status, metadata) = get_json(rest_app(state.clone(), caller.clone()), uri.clone()).await?;
    assert_eq!(status, StatusCode::OK, "{metadata}");
    assert_eq!(metadata, json!({ "metadata": {} }));

    for (method, body) in [
        ("PUT", json!({ "metadata": { "source": "user" } })),
        ("PATCH", json!({ "patch": { "source": "user" } })),
    ] {
        let (status, response) = json_request(
            rest_app(state.clone(), caller.clone()),
            method,
            uri.clone(),
            body,
        )
        .await?;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{response}");
    }

    db.cleanup().await;
    Ok(())
}
