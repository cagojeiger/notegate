#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::StatusCode;
use notegate_db::{LinkGraphProjectNodesJob, test_support::TestDb};
use notegate_jobs::{JobQueue, JobSpec};
use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
use notegate_service::files::{CreateText, WriteTarget, WriteText, WriteTextBody};
use serde_json::json;

use super::test_support::{caller_and_space, empty_request, get_json, rest_app, state};

#[tokio::test]
async fn rest_link_graph_routes_enforce_visibility_and_queue_manual_sync()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let text = state
        .files
        .create_text(
            owner.account_id(),
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "source.md".to_owned(),
            },
        )
        .await?;
    let node_id = text.node.node.id;
    state
        .files
        .write_text(
            owner.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id },
                body: WriteTextBody::Plain("[missing](./missing.md)".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;

    let (status, queued) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links/sync"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["status"], json!("queued"));
    assert!(queued.get("job_id").is_none());
    let (stored_job_id, payload): (uuid::Uuid, serde_json::Value) = sqlx::query_as(
        "SELECT job_id, payload FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await?;
    let (status, graph_state) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{graph_state}");
    assert_eq!(graph_state["status"], json!("syncing"));

    let node_ids = payload["node_ids"]
        .as_array()
        .expect("node ids")
        .iter()
        .map(|value| uuid::Uuid::parse_str(value.as_str().expect("node id")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut jobs = JobQueue::new(db.pool.clone())
        .claim_many(
            "link-graph-rest-test",
            &[LinkGraphProjectNodesJob::KIND.to_owned()],
            std::time::Duration::from_secs(300),
            1,
        )
        .await?;
    let claimed = jobs.pop().expect("projection job");
    assert_eq!(claimed.job_id, stored_job_id);
    state
        .link_graph
        .project_job(claimed.fence(), space_id, &node_ids)
        .await?;
    let (status, graph_state) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{graph_state}");
    assert_eq!(graph_state["status"], json!("idle"));
    assert!(graph_state["projected_at"].is_string());
    assert!(graph_state["failure_code"].is_null());
    assert!(graph_state["failed_at"].is_null());
    let (status, outgoing) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{outgoing}");
    assert_eq!(outgoing["links"][0]["path"], json!("/missing.md"));
    assert!(outgoing["links"][0]["node_id"].is_null());

    let (stranger_account, stranger_user) = state
        .accounts
        .upsert_user_by_sub(&ResolveAttrs {
            sub: "link-graph-stranger".to_owned(),
            email: "link-graph-stranger@example.test".to_owned(),
            name: "Link Graph Stranger".to_owned(),
        })
        .await?;
    let stranger = Caller {
        account: stranger_account,
        identity: CallerIdentity::User(stranger_user),
        channel: Channel::Browser,
    };
    let (status, hidden) = get_json(
        rest_app(state, stranger),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");

    db.cleanup().await;
    Ok(())
}
