#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::StatusCode;
use notegate_db::{FilesRepo, TextMutationKind, test_support::TestDb};
use notegate_model::files::{CreateFolder, MoveNode, StoredContent, WriteTextBody};
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
    let (second_source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source-2.md",
            &text("[target](target.md)"),
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
    assert!(source_links.get("outgoing").is_none());

    let (status, first_outgoing_page) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!(
            "/v1/spaces/{space_id}/nodes/{}/links/outgoing?limit=1",
            source.id
        ),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{first_outgoing_page}");
    assert_eq!(
        first_outgoing_page["links"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        first_outgoing_page["links"][0]["path"],
        json!("/missing.md")
    );
    assert_eq!(first_outgoing_page["links"][0]["node_id"], json!(null));
    assert_eq!(first_outgoing_page["page"]["has_more"], json!(true));
    let outgoing_cursor = first_outgoing_page["page"]["next_cursor"]
        .as_str()
        .ok_or("missing outgoing cursor")?;
    let (status, second_outgoing_page) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!(
            "/v1/spaces/{space_id}/nodes/{}/links/outgoing?limit=1&cursor={outgoing_cursor}",
            source.id
        ),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{second_outgoing_page}");
    assert_eq!(
        second_outgoing_page["links"][0]["path"],
        json!("/target.md")
    );
    assert_eq!(
        second_outgoing_page["links"][0]["node_id"],
        json!(target.id)
    );
    assert_eq!(second_outgoing_page["page"]["has_more"], json!(false));

    let (status, wrong_scope) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!(
            "/v1/spaces/{space_id}/nodes/{}/links/outgoing?cursor={outgoing_cursor}",
            target.id
        ),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{wrong_scope}");

    let (status, target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!(
            "/v1/spaces/{space_id}/nodes/{}/links/incoming?limit=1",
            target.id
        ),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{target_links}");
    assert_eq!(target_links["links"].as_array().map(Vec::len), Some(1));
    assert_eq!(target_links["page"]["has_more"], json!(true));
    let incoming_cursor = target_links["page"]["next_cursor"]
        .as_str()
        .ok_or("missing incoming cursor")?;
    let (status, next_target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!(
            "/v1/spaces/{space_id}/nodes/{}/links/incoming?limit=1&cursor={incoming_cursor}",
            target.id
        ),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{next_target_links}");
    let mut incoming_ids = vec![
        target_links["links"][0]["node_id"].to_string(),
        next_target_links["links"][0]["node_id"].to_string(),
    ];
    incoming_ids.sort();
    let mut expected_incoming_ids = vec![
        json!(source.id).to_string(),
        json!(second_source.id).to_string(),
    ];
    expected_incoming_ids.sort();
    assert_eq!(incoming_ids, expected_incoming_ids);
    assert_eq!(next_target_links["page"]["has_more"], json!(false));

    sqlx::query("DELETE FROM node_link_refs WHERE space_id = $1")
        .bind(space_id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "DELETE FROM reconciliation_work_items \
         WHERE work_kind = 'node_link_source' AND space_id = $1",
    )
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
        format!("/v1/spaces/{space_id}/nodes/{}/links/outgoing", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{rebuilt_source_links}");
    assert_eq!(
        rebuilt_source_links["links"].as_array().map(Vec::len),
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
    let (status, updated_incoming) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links/incoming", target.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{updated_incoming}");
    assert_eq!(updated_incoming["links"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        updated_incoming["links"][0]["node_id"],
        json!(second_source.id)
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

#[tokio::test]
async fn rest_link_index_converges_after_move_rename_and_delete()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, space_id, root_id) = caller_and_space(&state).await?;
    let files = FilesRepo::new(db.pool.clone());
    let folder = files
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "archive".to_owned(),
            },
            owner.account_id(),
        )
        .await?;
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
            &text("[target](target.md)"),
            owner.account_id(),
        )
        .await?;
    drain_link_index(&state).await?;

    files
        .move_node(
            space_id,
            &MoveNode {
                node_id: target.id,
                new_parent_node_id: folder.id,
                new_name: Some("renamed.md".to_owned()),
                expected_parent_id: Some(root_id),
            },
            owner.account_id(),
        )
        .await?;
    drain_link_index(&state).await?;

    let (status, moved_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links/outgoing", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{moved_links}");
    assert_eq!(moved_links["links"][0]["path"], json!("/target.md"));
    assert_eq!(moved_links["links"][0]["node_id"], json!(null));

    files
        .save_text_content(
            space_id,
            source.id,
            &text("[target](archive/renamed.md)"),
            None,
            owner.account_id(),
            TextMutationKind::Write,
        )
        .await?;
    drain_link_index(&state).await?;
    let (status, target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links/incoming", target.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{target_links}");
    assert_eq!(target_links["links"].as_array().map(Vec::len), Some(1));
    assert_eq!(target_links["links"][0]["node_id"], json!(source.id));

    files
        .soft_delete_node(space_id, target.id, owner.account_id(), false)
        .await?;
    drain_link_index(&state).await?;
    let (status, deleted_target_links) = get_json(
        rest_app(state.clone(), owner.clone()),
        format!("/v1/spaces/{space_id}/nodes/{}/links/outgoing", source.id),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{deleted_target_links}");
    assert_eq!(
        deleted_target_links["links"][0]["path"],
        json!("/archive/renamed.md")
    );
    assert_eq!(deleted_target_links["links"][0]["node_id"], json!(null));

    files
        .soft_delete_node(space_id, source.id, owner.account_id(), false)
        .await?;
    drain_link_index(&state).await?;
    let remaining_refs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM node_link_refs WHERE space_id = $1 AND source_node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&state.db)
    .await?;
    let remaining_state: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM reconciliation_work_items \
         WHERE work_kind = 'node_link_source' AND space_id = $1 AND target_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&state.db)
    .await?;
    assert_eq!(remaining_refs, 0);
    assert_eq!(remaining_state, 0);

    db.cleanup().await;
    Ok(())
}
