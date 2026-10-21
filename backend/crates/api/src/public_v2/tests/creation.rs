use axum::http::StatusCode;
use notegate_db::test_support::TestDb;
use notegate_model::Permission;
use notegate_service::ServiceError;
use notegate_service::files::{ReadText, ReadTextBody};
use serde_json::json;
use uuid::Uuid;

use super::{agent_caller, app};
use crate::rest::test_support::{caller_and_space, json_request, rest_app, state};

#[tokio::test]
async fn node_creation_rules_match_across_v1_and_v2() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let agent = agent_caller(&state, owner.account_id(), space_id, Permission::Write).await?;

    for (version, caller) in [("v1", owner.clone()), ("v2", agent)] {
        for (index, (kind, content, status)) in [
            ("folder", None, StatusCode::CREATED),
            ("folder", Some(""), StatusCode::BAD_REQUEST),
            ("folder", Some("discarded before"), StatusCode::BAD_REQUEST),
            ("text", None, StatusCode::CREATED),
            ("text", Some(""), StatusCode::CREATED),
            ("text", Some("initial content\n"), StatusCode::CREATED),
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("{version}-{index}.md");
            let mut body = json!({"parent_id": root_id, "kind": kind, "name": name});
            if let Some(content) = content {
                body["content"] = json!(content);
            }
            let (router, path) = if version == "v1" {
                (
                    rest_app(state.clone(), caller.clone()),
                    format!("/v1/spaces/{space_id}/nodes"),
                )
            } else {
                (
                    app(state.clone(), caller.clone()),
                    format!("/spaces/{space_id}/nodes"),
                )
            };
            let (actual, response) = json_request(router, "POST", path, body).await?;
            assert_eq!(actual, status, "{version}: {response}");
            if status == StatusCode::BAD_REQUEST {
                assert_eq!(response["error"], "invalid_input");
                assert_eq!(response["message"], "content is only valid when kind=text");
                assert!(matches!(
                    state
                        .files
                        .resolve_path(owner.account_id(), space_id, &format!("/{name}"))
                        .await,
                    Err(ServiceError::NotFound(_))
                ));
                continue;
            }
            assert_eq!(response["kind"], kind);
            let node_id: Uuid = serde_json::from_value(response["id"].clone())?;
            let node = state
                .files
                .stat(owner.account_id(), space_id, node_id)
                .await?;
            assert_eq!(node.node.created_by_account_id, caller.account_id());
            if kind == "text" {
                let read = state
                    .files
                    .read_text(
                        owner.account_id(),
                        space_id,
                        ReadText {
                            node_id,
                            start_line: None,
                            max_lines: None,
                            max_bytes: None,
                            if_none_match_sha256: None,
                        },
                    )
                    .await?;
                let ReadTextBody::Content(text) = read.body else {
                    panic!("created text must be readable");
                };
                assert_eq!(text.content, content.unwrap_or_default());
            }
        }
    }
    db.cleanup().await;
    Ok(())
}
