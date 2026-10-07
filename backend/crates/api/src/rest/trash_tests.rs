#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
use axum::http::StatusCode;
use notegate_db::{FilesRepo, test_support::TestDb};
use notegate_model::{Channel, files::CreateFolder};
use tower::ServiceExt as _;

use super::test_support::{caller_and_space, empty_request, get_json, rest_app, state};

#[tokio::test]
async fn dashboard_trash_lists_restores_and_returns_accepted_for_permanent_deletion()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space, root) = caller_and_space(&state).await?;
    let repo = FilesRepo::with_limits_and_crypto(
        db.pool.clone(),
        state.config.limits,
        state.security.clone(),
    );
    let item = repo
        .insert_folder(
            space,
            &CreateFolder {
                parent_node_id: root,
                name: "deleted".to_owned(),
            },
            caller.account_id(),
        )
        .await?;
    repo.soft_delete_node(space, item.id, caller.account_id(), true)
        .await?;
    let (status, list) = get_json(
        rest_app(state.clone(), caller.clone()),
        "/v1/me/trash?limit=1".to_owned(),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"][0]["id"], item.id.to_string());
    assert_eq!(list["items"][0]["recoverable"], true);
    let deletion =
        uuid::Uuid::parse_str(list["items"][0]["deletion_operation_id"].as_str().unwrap())?;
    let path = format!("/v1/me/trash/spaces/{space}/nodes/{}", item.id);
    let first_version = version_query(&list["items"][0]);
    let (status, _) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("{path}/restore?{first_version}"),
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, events) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space}/file-change-events?node_id={}", item.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(events["events"][0]["op_type"], "item.restore");
    assert_eq!(
        events["events"][0]["metadata"]["related_deletion_operation_id"],
        deletion.to_string()
    );
    assert_ne!(
        events["events"][0]["operation_id"],
        events["events"][1]["operation_id"]
    );
    assert_eq!(events["events"][1]["operation_id"], deletion.to_string());
    repo.soft_delete_node(space, item.id, caller.account_id(), true)
        .await?;
    for (method, route) in [
        ("POST", format!("{path}/restore?{first_version}")),
        ("DELETE", format!("{path}?{first_version}")),
    ] {
        let (status, _) =
            empty_request(rest_app(state.clone(), caller.clone()), method, route).await?;
        assert_eq!(status, StatusCode::CONFLICT);
    }
    // Extractor rejection is plain text, unlike the domain error JSON envelope.
    let rejected = rest_app(state.clone(), caller.clone())
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri(&path)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let (_, current) = get_json(
        rest_app(state.clone(), caller.clone()),
        "/v1/me/trash".to_owned(),
    )
    .await?;
    assert_eq!(current["items"][0]["recoverable"], true);
    assert_eq!(current["items"][0]["deletion_pending"], false);
    let current_version = version_query(&current["items"][0]);
    let (status, result) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "DELETE",
        format!("{path}?{current_version}"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(result["status"], "deletion_requested");
    let (status, _) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("{path}/restore?{current_version}"),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, audit) = get_json(
        rest_app(state.clone(), caller.clone()),
        "/v1/me/audit-events".to_owned(),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit["events"][0]["op_type"], "trash.purge.request");
    uuid::Uuid::parse_str(audit["events"][0]["operation_id"].as_str().unwrap())?;
    let (status, list) = get_json(rest_app(state, caller), "/v1/me/trash".to_owned()).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"][0]["deletion_pending"], true);
    assert_ne!(
        list["items"][0]["deletion_operation_id"],
        deletion.to_string()
    );
    assert_eq!(
        audit["events"][0]["metadata"]["related_deletion_operation_id"],
        list["items"][0]["deletion_operation_id"]
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn trash_cursor_is_owner_scoped_and_external_channels_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space, root) = caller_and_space(&state).await?;
    let repo = FilesRepo::with_limits_and_crypto(
        db.pool.clone(),
        state.config.limits,
        state.security.clone(),
    );
    for name in ["first", "second"] {
        let item = repo
            .insert_folder(
                space,
                &CreateFolder {
                    parent_node_id: root,
                    name: name.to_owned(),
                },
                caller.account_id(),
            )
            .await?;
        repo.soft_delete_node(space, item.id, caller.account_id(), true)
            .await?;
    }
    let page = state.files.list_trash(&caller, Some(1), None).await?;
    assert!(page.has_more);
    let second = state
        .files
        .list_trash(&caller, Some(1), page.next_cursor.as_deref())
        .await?;
    assert_ne!(page.items[0].id, second.items[0].id);
    assert!(!second.has_more);
    let mut external = caller.clone();
    external.channel = Channel::Api;
    assert!(state.files.list_trash(&external, None, None).await.is_err());
    assert!(
        state
            .files
            .restore_trash(
                &external,
                space,
                Some(page.items[0].id),
                (&page.items[0]).into()
            )
            .await
            .is_err()
    );
    let mut other = caller.clone();
    other.account.id = uuid::Uuid::new_v4();
    assert!(
        state
            .files
            .list_trash(&other, Some(1), page.next_cursor.as_deref())
            .await
            .is_err()
    );
    assert!(
        state
            .files
            .list_trash(&caller, None, Some("invalid-cursor"))
            .await
            .is_err()
    );
    db.cleanup().await;
    Ok(())
}

fn version_query(item: &serde_json::Value) -> String {
    let mut query = format!("deleted_at={}", item["deleted_at"].as_str().unwrap());
    if let Some(operation) = item["deletion_operation_id"].as_str() {
        query.push_str(&format!("&deletion_operation_id={operation}"));
    }
    query
}
