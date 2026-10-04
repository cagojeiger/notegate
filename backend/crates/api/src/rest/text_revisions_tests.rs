#![allow(
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]
use super::test_support::{
    caller_and_space, get_json, json_request, json_response, rest_app, state,
};
use axum::http::StatusCode;
use notegate_db::test_support::TestDb;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn history_http_contract_paginates_and_restores_with_a_required_guard()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space, root) = caller_and_space(&state).await?;
    let app = rest_app(state.clone(), caller.clone());
    let (status, created) = json_request(
        app.clone(),
        "POST",
        format!("/v1/spaces/{space}/nodes"),
        json!({"parent_id":root,"kind":"text","name":"history.md"}),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let node: Uuid = serde_json::from_value(created["id"].clone())?;
    let uri = format!("/v1/spaces/{space}/text/{node}");
    let history = format!("{uri}/revisions");
    let session = Uuid::new_v4();
    let mut current = String::new();
    for content in ["first", "second", "third"] {
        let (status, written) = json_request(
            app.clone(),
            "PUT",
            uri.clone(),
            json!({"content":content,"edit_session_id":session}),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{written}");
        current = written["text"]["content_sha256"]
            .as_str()
            .unwrap()
            .to_owned();
    }
    let (status, page) = get_json(app.clone(), format!("{history}?limit=1")).await?;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["revisions"].as_array().unwrap().len(), 1);
    assert!(page["revisions"][0].get("content").is_none());
    let id = page["revisions"][0]["id"].as_str().unwrap();
    let (status, body) = get_json(app.clone(), format!("{history}/{id}")).await?;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["content"], "second");
    let (_, next) = get_json(
        app.clone(),
        format!(
            "{history}?limit=1&cursor={}",
            page["next_cursor"].as_str().unwrap()
        ),
    )
    .await?;
    assert_ne!(next["revisions"][0]["id"], page["revisions"][0]["id"]);
    let (status, _) = get_json(app.clone(), format!("{history}?limit=101")).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response = json_response(
        app.clone(),
        "POST",
        format!("{history}/{id}/restore"),
        json!({}),
    )
    .await?;
    assert!(response.status().is_client_error());
    let (status, _) = json_request(
        app.clone(),
        "POST",
        format!("{history}/{id}/restore"),
        json!({"expected_sha256":"0".repeat(64)}),
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, restored) = json_request(
        app.clone(),
        "POST",
        format!("{history}/{id}/restore"),
        json!({"expected_sha256":current}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{restored}");
    let (_, new_page) = get_json(app.clone(), history.clone()).await?;
    assert_eq!(new_page["revisions"].as_array().unwrap().len(), 4);
    assert_eq!(new_page["revisions"][0]["content_sha256"], current);
    let (status, _) = json_request(
        app.clone(),
        "POST",
        format!("{history}/{id}/restore"),
        json!({"expected_sha256":restored["content_sha256"]}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (_, after_noop) = get_json(app, history).await?;
    assert_eq!(after_noop["revisions"].as_array().unwrap().len(), 4);
    db.cleanup().await;
    Ok(())
}
