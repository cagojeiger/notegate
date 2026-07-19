#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::StatusCode;
use notegate_db::{AccountRepo, FilesRepo, test_support::TestDb};
use notegate_model::files::BeginObjectUpload;
use notegate_model::{Caller, CallerIdentity, Channel, FileEncryptionMode, ResolveAttrs};
use serde_json::json;
use uuid::Uuid;

use crate::rest::test_support::{
    caller_and_space, empty_request, get_json, json_request, rest_app, state,
};

#[tokio::test]
async fn rest_file_change_events_capture_and_list_real_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let owner = caller.account_id();

    let (status, folder) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "kind": "folder",
            "name": "docs"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{folder}");
    let folder_id: Uuid = serde_json::from_value(folder["id"].clone())?;

    let (status, text) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "kind": "text",
            "name": "note.md",
            "content": "hello"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let text_id: Uuid = serde_json::from_value(text["id"].clone())?;

    let upload_id = Uuid::new_v4();
    let files = FilesRepo::new(db.pool.clone());
    files
        .insert_object_upload(
            upload_id,
            &format!("objects/{upload_id}"),
            space_id,
            owner,
            &BeginObjectUpload {
                parent_node_id: root_id,
                name: "asset.txt".to_owned(),
                byte_len: 5,
                media_type: "text/plain".to_owned(),
                original_filename: Some("asset.txt".to_owned()),
                encryption_mode: FileEncryptionMode::None,
                encryption_metadata: None,
            },
        )
        .await?;
    let (file_node, _) = files
        .attach_object_upload(upload_id, space_id, owner)
        .await?;
    let file_node_id = file_node.id;

    let (status, written) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/text/{text_id}"),
        json!({
            "storage_format": "plain",
            "content": "hello world"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{written}");

    let (status, no_op_write) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/text/{text_id}"),
        json!({
            "storage_format": "plain",
            "content": "hello world"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_write}");

    let (status, metadata) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/metadata"),
        json!({
            "metadata": {"source": "rest-e2e"}
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{metadata}");

    let (status, no_op_metadata) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/metadata"),
        json!({
            "metadata": {"source": "rest-e2e"}
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_metadata}");

    let (status, updated) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PATCH",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
        json!({
            "name": "renamed.md",
            "sort_order": 10
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, no_op_update) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PATCH",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
        json!({
            "name": "renamed.md",
            "sort_order": 10
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_update}");

    let (status, moved) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/move"),
        json!({
            "new_parent_id": folder_id,
            "expected_parent_id": root_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{moved}");

    let (status, no_op_move) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/move"),
        json!({
            "new_parent_id": folder_id,
            "expected_parent_id": folder_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_move}");

    let (status, deleted) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "DELETE",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT, "{deleted}");

    let (status, events) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{events}");
    let op_types: Vec<_> = events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|event| event["op_type"].as_str().expect("op_type"))
        .collect();
    assert_eq!(
        op_types,
        vec![
            "item.delete",
            "item.move",
            "item.update",
            "metadata.replace",
            "text.write",
            "file.create",
            "text.create",
            "folder.create",
        ]
    );
    assert!(
        events["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["actor_account_id"] == json!(owner))
    );
    assert!(
        events["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["actor"]["id"] == json!(owner))
    );
    let file_event = events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .find(|event| event["node_id"] == json!(file_node_id))
        .expect("file event");
    assert_eq!(file_event["metadata"]["byte_len_after"], json!(5));
    assert_eq!(file_event["metadata"]["item_name"], json!("asset.txt"));
    assert!(file_event["metadata"].get("line_count_after").is_none());

    let (status, first_page) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=3"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{first_page}");
    assert_eq!(first_page["page"]["has_more"], json!(true));
    let cursor = first_page["page"]["next_cursor"]
        .as_str()
        .expect("next cursor");
    let (status, second_page) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=3&cursor={cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{second_page}");
    assert!(
        second_page["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["op_type"] != "item.delete")
    );

    let (status, text_events) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?node_id={text_id}&limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{text_events}");
    assert_eq!(
        text_events["events"]
            .as_array()
            .expect("events array")
            .len(),
        6
    );
    let text_event_ops: Vec<_> = text_events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|event| event["op_type"].as_str().expect("op_type"))
        .collect();
    assert_eq!(
        text_event_ops,
        vec![
            "item.delete",
            "item.move",
            "item.update",
            "metadata.replace",
            "text.write",
            "text.create",
        ]
    );
    assert!(
        text_events["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["node_id"] == json!(text_id))
    );

    let (status, invalid_cursor) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?cursor=not-a-cursor"),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid_cursor}");

    let (stranger_account, stranger_user) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "rest-events-stranger".to_owned(),
        email: "stranger@example.test".to_owned(),
        name: "Stranger".to_owned(),
    })
    .await?;
    let stranger = Caller {
        account: stranger_account,
        identity: CallerIdentity::User(stranger_user),
        channel: Channel::Browser,
    };
    let (status, hidden) = get_json(
        rest_app(state.clone(), stranger),
        format!("/v1/spaces/{space_id}/file-change-events?limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");

    db.cleanup().await;
    Ok(())
}
