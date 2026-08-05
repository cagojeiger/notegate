//! Integration tests for eventually consistent link indexing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, space_with_root};
use notegate_db::{
    FilesRepo, LinkIndexRepo, MetadataMutationKind, StoredLinkReference, TextMutationKind,
};
use notegate_model::LinkReferenceKind;
use notegate_model::files::{StoredContent, UpdateNode, WriteTextBody};
use serde_json::json;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: "0".repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

#[tokio::test]
async fn file_changes_enqueue_only_the_required_scope() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-scope").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space_id, root_id, "note.md", &text("before"), account_id)
        .await?;

    assert!(links.source_state(space_id, node.id).await?.is_none());
    let space_version: i64 = sqlx::query_scalar(
        "SELECT requested_version FROM node_link_space_reindex_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;

    files
        .save_text_content(
            space_id,
            node.id,
            &text("after"),
            None,
            account_id,
            TextMutationKind::Write,
        )
        .await?;
    let source_state = links
        .source_state(space_id, node.id)
        .await?
        .expect("text write should enqueue its source");
    assert_eq!(source_state.requested_version, 1);

    files
        .replace_node_metadata(
            space_id,
            node.id,
            &json!({"owner": "docs"}),
            account_id,
            MetadataMutationKind::Replace,
        )
        .await?;
    files
        .update_node(
            space_id,
            &UpdateNode {
                node_id: node.id,
                name: None,
                sort_order: Some(10),
            },
            account_id,
        )
        .await?;
    assert_eq!(
        links
            .source_state(space_id, node.id)
            .await?
            .expect("source state")
            .requested_version,
        1,
        "metadata and display order do not affect links"
    );
    let unchanged_space_version: i64 = sqlx::query_scalar(
        "SELECT requested_version FROM node_link_space_reindex_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(unchanged_space_version, space_version);

    files
        .update_node(
            space_id,
            &UpdateNode {
                node_id: node.id,
                name: Some("renamed.md".to_owned()),
                sort_order: None,
            },
            account_id,
        )
        .await?;
    let renamed_space_version: i64 = sqlx::query_scalar(
        "SELECT requested_version FROM node_link_space_reindex_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(renamed_space_version, space_version + 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failed_link_enqueue_rolls_back_the_document_change()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-atomic-change").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space_id, root_id, "note.md", &text("before"), account_id)
        .await?;
    links.request_source(space_id, node.id).await?;
    sqlx::query(
        "UPDATE node_link_source_states SET requested_version = 9223372036854775807 \
         WHERE space_id = $1 AND source_node_id = $2",
    )
    .bind(space_id)
    .bind(node.id)
    .execute(&db.pool)
    .await?;
    let event_count_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_change_events WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;

    assert!(
        files
            .save_text_content(
                space_id,
                node.id,
                &text("after"),
                None,
                account_id,
                TextMutationKind::Write,
            )
            .await
            .is_err()
    );
    let (_, stored) = files
        .find_text(space_id, node.id)
        .await?
        .expect("text should still exist");
    assert_eq!(stored.content.as_deref(), Some("before"));
    let event_count_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM file_change_events WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(event_count_after, event_count_before);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn source_replacement_is_atomic_versioned_and_fenced()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-replace").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), account_id)
        .await?;

    links.request_source(space_id, source.id).await?;
    let initial_claim = links.claim_source().await?.expect("initial claim");
    let initial = StoredLinkReference {
        target_node_id: Some(target.id),
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    };
    assert!(
        links
            .complete_source(&initial_claim, std::slice::from_ref(&initial))
            .await?
    );

    links.request_source(space_id, source.id).await?;
    let replacement_claim = links.claim_source().await?.expect("replacement claim");
    let invalid = StoredLinkReference {
        occurrence_count: 0,
        ..initial.clone()
    };
    assert!(
        links
            .complete_source(&replacement_claim, &[invalid])
            .await
            .is_err()
    );
    assert_eq!(
        links.outgoing(space_id, source.id).await?,
        vec![initial.clone()]
    );
    let rolled_back_state = links
        .source_state(space_id, source.id)
        .await?
        .expect("source state");
    assert_eq!(
        rolled_back_state.applied_version,
        initial_claim.requested_version
    );
    assert_eq!(
        rolled_back_state.requested_version,
        replacement_claim.requested_version
    );

    let replacement = StoredLinkReference {
        target_path: "/renamed-target.md".to_owned(),
        ..initial.clone()
    };
    assert!(
        links
            .complete_source(&replacement_claim, std::slice::from_ref(&replacement))
            .await?
    );
    assert_eq!(
        links.outgoing(space_id, source.id).await?,
        vec![replacement]
    );

    links.request_source(space_id, source.id).await?;
    let stale_claim = links.claim_source().await?.expect("stale claim");
    sqlx::query(
        "UPDATE node_link_source_states SET claim_until = now() - INTERVAL '1 second' \
         WHERE space_id = $1 AND source_node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .execute(&db.pool)
    .await?;
    let current_claim = links
        .claim_source()
        .await?
        .expect("replacement worker claim");
    assert_ne!(stale_claim.claim_token, current_claim.claim_token);
    assert!(!links.complete_source(&stale_claim, &[]).await?);
    assert!(links.complete_source(&current_claim, &[]).await?);

    links.request_source(space_id, source.id).await?;
    let active_claim = links.claim_source().await?.expect("active claim");
    links.request_source(space_id, source.id).await?;
    assert!(links.complete_source(&active_claim, &[]).await?);
    let pending_state = links
        .source_state(space_id, source.id)
        .await?
        .expect("pending source state");
    assert!(pending_state.requested_version > pending_state.applied_version);
    let final_claim = links.claim_source().await?.expect("final claim");
    assert!(links.complete_source(&final_claim, &[]).await?);
    let final_state = links
        .source_state(space_id, source.id)
        .await?
        .expect("final source state");
    assert_eq!(final_state.requested_version, final_state.applied_version);

    db.cleanup().await;
    Ok(())
}
