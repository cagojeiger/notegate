#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::StatusCode;
use notegate_db::{FilesRepo, TextMutationKind, test_support::TestDb};
use notegate_model::files::{StoredContent, WriteTextBody};
use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
use serde_json::json;

use super::test_support::{caller_and_space, empty_request, get_json, rest_app, state};

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

async fn drain_link_index(
    state: &crate::state::AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..20 {
        if matches!(
            state.link_index.execute_next().await?,
            notegate_service::link_index::LinkIndexExecution::Idle
        ) {
            return Ok(());
        }
    }
    Err("link index did not drain".into())
}

#[tokio::test]
async fn rest_link_index_rebuilds_relationships_and_enforces_space_access()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let files = FilesRepo::new(db.pool.clone());
    let (target, _) = files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target"),
            owner.account_id(),
        )
        .await?;
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](target.md) [missing](missing.md)"),
            owner.account_id(),
        )
        .await?;

    drain_link_index(&state).await?;

    let (status, source_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{source_links}");
    assert_eq!(source_links["status"], json!("up_to_date"));
    assert_eq!(source_links["outgoing"].as_array().map(Vec::len), Some(2));
    assert_eq!(source_links["outgoing"][0]["path"], json!("/missing.md"));
    assert_eq!(source_links["outgoing"][0]["node_id"], json!(null));
    assert_eq!(source_links["outgoing"][1]["path"], json!("/target.md"));
    assert_eq!(source_links["outgoing"][1]["node_id"], json!(target.id));

    let (status, target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links", target.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{target_links}");
    assert_eq!(target_links["incoming"].as_array().map(Vec::len), Some(1));
    assert_eq!(target_links["incoming"][0]["node_id"], json!(source.id));
    assert_eq!(target_links["incoming"][0]["path"], json!("/source.md"));

    sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1")
        .bind(space_id)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM node_link_source_states WHERE space_id = $1")
        .bind(space_id)
        .execute(&state.db)
        .await?;
    let (status, queued) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/link-index/reindex"),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["status"], json!("queued"));
    drain_link_index(&state).await?;
    let (status, rebuilt_source_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{rebuilt_source_links}");
    assert_eq!(
        rebuilt_source_links["outgoing"].as_array().map(Vec::len),
        Some(2)
    );

    files
        .save_text_content(
            space_id,
            source.id,
            &text("[missing](missing.md)"),
            None,
            owner.account_id(),
            TextMutationKind::Write,
        )
        .await?;
    let (status, pending_target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links", target.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{pending_target_links}");
    assert_eq!(pending_target_links["status"], json!("pending"));

    assert!(matches!(
        state.link_index.execute_next().await?,
        notegate_service::link_index::LinkIndexExecution::SourceIndexed { .. }
    ));
    let (status, updated_target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links", target.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{updated_target_links}");
    assert_eq!(updated_target_links["status"], json!("up_to_date"));
    assert_eq!(
        updated_target_links["incoming"].as_array().map(Vec::len),
        Some(0)
    );

    let (status, queued) = empty_request(
        rest_app(state.clone(), owner.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{}/links/sync", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    assert_eq!(queued["status"], json!("queued"));

    let (stranger_account, stranger_user) = state
        .accounts
        .upsert_user_by_sub(&ResolveAttrs {
            sub: "rest-link-index-stranger".to_owned(),
            email: "rest-link-index-stranger@example.test".to_owned(),
            name: "REST Link Index Stranger".to_owned(),
        })
        .await?;
    let stranger = Caller {
        account: stranger_account,
        identity: CallerIdentity::User(stranger_user),
        channel: Channel::Browser,
    };
    let (status, hidden) = get_json(
        rest_app(state, stranger),
        format!("/v1/spaces/{space_id}/nodes/{}/links", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");

    db.cleanup().await;
    Ok(())
}
