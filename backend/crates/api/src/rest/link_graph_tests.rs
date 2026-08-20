#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::{StatusCode, header::LOCATION};
use notegate_db::{
    LINK_GRAPH_ACTIVE_JOB_MAX, LinkGraphProjectNodesJob, LinkGraphProjectNodesPayload,
    test_support::TestDb,
};
use notegate_jobs::{JobQueue, JobSpec};
use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
use notegate_service::files::{CreateText, WriteTarget, WriteText, WriteTextBody};
use serde_json::json;

use super::test_support::{
    caller_and_space, decode_response, empty_request, get_json, json_response, rest_app, state,
};

#[tokio::test]
async fn rest_link_graph_routes_enforce_visibility_and_accept_manual_sync()
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

    let (status, unsupported) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{root_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{unsupported}");
    assert_eq!(unsupported["availability"]["reason"], json!("unsupported"));

    let accepted_response = json_response(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links"),
        json!(null),
    )
    .await?;
    assert_eq!(
        accepted_response.headers()[LOCATION],
        format!("/api/v1/spaces/{space_id}/nodes/{node_id}/links")
    );
    let (status, accepted) = decode_response(accepted_response).await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["result"], json!("accepted"));
    assert_eq!(accepted["availability"]["reason"], json!("pending"));
    assert!(accepted.get("job_id").is_none());

    let (status, duplicate) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{duplicate}");
    assert_eq!(duplicate["result"], json!("already_pending"));

    let (status, index_status) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/link-index"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{index_status}");
    assert_eq!(index_status["status"], json!("pending"));
    assert_eq!(index_status["availability"]["reason"], json!("pending"));
    let (stored_job_id, payload, history_visibility, history_owner_account_id): (
        uuid::Uuid,
        serde_json::Value,
        String,
        Option<uuid::Uuid>,
    ) = sqlx::query_as(
        "SELECT job_id, payload, history_visibility, history_owner_account_id \
         FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(history_visibility, "visible");
    assert_eq!(history_owner_account_id, Some(owner.account_id()));
    let (status, graph_state) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{graph_state}");
    assert_eq!(graph_state["status"], json!("syncing"));
    assert_eq!(graph_state["space_pending"], json!(true));
    assert_eq!(graph_state["availability"]["reason"], json!("pending"));

    let sources = serde_json::from_value::<LinkGraphProjectNodesPayload>(payload)?.sources;
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
        .project_job(claimed.fence(), space_id, &sources)
        .await?;
    let (status, graph_state) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{graph_state}");
    assert_eq!(graph_state["status"], json!("idle"));
    assert_eq!(graph_state["space_pending"], json!(true));
    assert!(graph_state["projected_at"].is_string());
    assert!(graph_state["failure_code"].is_null());
    assert!(graph_state["failed_at"].is_null());
    assert_eq!(graph_state["availability"]["can_trigger"], json!(true));
    let (status, outgoing) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links/outgoing"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{outgoing}");
    assert_eq!(outgoing["links"][0]["path"], json!("/missing.md"));
    assert!(outgoing["links"][0]["node_id"].is_null());

    let accepted_response = json_response(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
        json!(null),
    )
    .await?;
    assert_eq!(
        accepted_response.headers()[LOCATION],
        format!("/api/v1/spaces/{space_id}/link-index")
    );
    let (status, accepted) = decode_response(accepted_response).await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["result"], json!("accepted"));
    assert!(accepted.get("job_id").is_none());

    let (status, duplicate) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{duplicate}");
    assert_eq!(duplicate["result"], json!("already_pending"));
    assert!(duplicate.get("job_id").is_none());

    let mut api_owner = owner.clone();
    api_owner.channel = Channel::Api;
    let (status, unavailable) = get_json(
        rest_app(state.clone(), api_owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{unavailable}");
    assert_eq!(unavailable["availability"]["reason"], json!("forbidden"));
    for path in [
        format!("/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links"),
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
    ] {
        let (status, forbidden) =
            empty_request(rest_app(state.clone(), api_owner.clone()), "POST", path).await?;
        assert_eq!(status, StatusCode::FORBIDDEN, "{forbidden}");
    }

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
    for path in [
        format!("/v1/spaces/{space_id}/nodes/{node_id}/actions/reindex-links"),
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
    ] {
        let (status, hidden) =
            empty_request(rest_app(state.clone(), stranger.clone()), "POST", path).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");
    }
    let (status, hidden) = get_json(
        rest_app(state, stranger),
        format!("/v1/spaces/{space_id}/nodes/{node_id}/links"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rest_reindex_stays_pending_when_projection_job_capacity_is_saturated()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    state
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
    sqlx::query(
        "INSERT INTO background_jobs (job_kind, payload) \
         SELECT $1, '{}'::jsonb FROM generate_series(1::bigint, $2)",
    )
    .bind(LinkGraphProjectNodesJob::KIND)
    .bind(LINK_GRAPH_ACTIVE_JOB_MAX)
    .execute(&db.pool)
    .await?;

    let (status, accepted) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["result"], json!("accepted"));

    let staged_without_job: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projections \
             WHERE space_id = $1 AND needs_projection \
               AND active_job_id IS NULL AND failed_at IS NULL \
         )",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(staged_without_job);
    let (status, index_status) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/link-index"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{index_status}");
    assert_eq!(index_status["status"], json!("pending"));

    let (status, duplicate) = empty_request(
        rest_app(state, owner),
        "POST",
        format!("/v1/spaces/{space_id}/actions/reindex-links"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{duplicate}");
    assert_eq!(duplicate["result"], json!("already_pending"));

    db.cleanup().await;
    Ok(())
}
