//! Postgres integration coverage for the eventually consistent Markdown link projection.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

mod common;

use std::time::Duration;

use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{FilesRepo, LinkIndexRepo, SpaceRepo, TextMutationKind};
use notegate_model::files::{StoredContent, WriteTextBody};
use notegate_model::{LinkIndexFreshness, LinkReferenceStatus};
use notegate_service::link_index::{LinkIndexRun, LinkIndexService};
use sha2::{Digest, Sha256};

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

async fn drain(
    service: &LinkIndexService,
) -> Result<Vec<LinkIndexRun>, Box<dyn std::error::Error>> {
    let mut runs = Vec::new();
    for _ in 0..8 {
        match service.process_next().await? {
            LinkIndexRun::Idle => return Ok(runs),
            run => runs.push(run),
        }
    }
    panic!("link index did not become idle");
}

#[tokio::test]
async fn projects_current_links_across_rewrites_deletes_and_recreation()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-lifecycle",
        "link-index-lifecycle@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-lifecycle").await;
    let files = FilesRepo::new(db.pool.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let service = LinkIndexService::new(index, files.clone());

    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), owner)
        .await?;
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](target.md) [again](target.md) ![missing](missing.png)"),
            owner,
        )
        .await?;

    let runs = drain(&service).await?;
    assert!(matches!(runs.as_slice(), [LinkIndexRun::Rebuilt { .. }]));

    let source_links = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(source_links.index.freshness(), LinkIndexFreshness::Current);
    assert_eq!(source_links.outgoing_count, 2);
    assert_eq!(source_links.broken_count, 1);
    assert_eq!(
        source_links
            .outgoing
            .iter()
            .find(|reference| reference.raw_href == "target.md")
            .expect("target reference")
            .occurrence_count,
        2
    );
    let target_links = service.node_links(owner, space_id, target.id).await?;
    assert_eq!(target_links.incoming_count, 1);

    let (latest, _) = files
        .insert_text(space_id, root_id, "latest.md", &text("latest"), owner)
        .await?;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("[obsolete](target.md)"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("[latest](latest.md)"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;

    drain(&service).await?;
    let rewritten = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(rewritten.outgoing_count, 1);
    assert_eq!(rewritten.outgoing[0].raw_href, "latest.md");
    assert_eq!(rewritten.outgoing[0].target_node_id, Some(latest.id));
    assert_eq!(
        service
            .node_links(owner, space_id, target.id)
            .await?
            .incoming_count,
        0
    );

    files
        .soft_delete_node(space_id, latest.id, owner, false)
        .await?;
    drain(&service).await?;
    let deleted_target = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(deleted_target.broken_count, 1);
    assert_eq!(
        deleted_target.outgoing[0].status,
        LinkReferenceStatus::Deleted
    );

    let (replacement, _) = files
        .insert_text(space_id, root_id, "latest.md", &text("replacement"), owner)
        .await?;
    drain(&service).await?;
    let rebound = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(rebound.broken_count, 0);
    assert_eq!(rebound.outgoing[0].status, LinkReferenceStatus::Resolved);
    assert_eq!(rebound.outgoing[0].target_node_id, Some(replacement.id));

    files
        .soft_delete_node(space_id, source.id, owner, false)
        .await?;
    drain(&service).await?;
    assert_eq!(
        service
            .node_links(owner, space_id, replacement.id)
            .await?
            .incoming_count,
        0
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn only_one_worker_claims_a_space() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-claim",
        "link-index-claim@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, _root_id) = setup_space(&spaces, owner, "link-index-claim").await;
    let index = LinkIndexRepo::new(db.pool.clone());

    let first = index
        .claim_next(Duration::from_secs(30), 1)
        .await?
        .expect("first worker claims queued space");
    assert_eq!(first.space_id, space_id);
    assert!(
        index
            .claim_next(Duration::from_secs(30), 1)
            .await?
            .is_none()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn an_expired_claim_cannot_commit_and_the_space_is_reclaimable()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-expired-claim",
        "link-index-expired-claim@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, _root_id) = setup_space(&spaces, owner, "link-index-expired-claim").await;
    let index = LinkIndexRepo::new(db.pool.clone());

    let expired = index
        .claim_next(Duration::ZERO, 1)
        .await?
        .expect("queued space is claimed");
    assert!(matches!(
        index
            .begin_rebuild(&expired, 1, Duration::from_secs(30))
            .await,
        Err(notegate_core::Error::Conflict(_))
    ));
    let replacement = index
        .claim_next(Duration::from_secs(30), 1)
        .await?
        .expect("expired claim is reclaimable");
    assert_eq!(replacement.space_id, space_id);
    assert_ne!(replacement.token, expired.token);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_generations_do_not_follow_global_event_id_order()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-generation-order",
        "link-index-generation-order@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-generation-order").await;
    let service = LinkIndexService::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );
    drain(&service).await?;

    let lower_id: i64 =
        sqlx::query_scalar("SELECT nextval(pg_get_serial_sequence('file_change_events', 'id'))")
            .fetch_one(&db.pool)
            .await?;
    let (higher_id, first_generation): (i64, i64) = sqlx::query_as(
        "INSERT INTO file_change_events \
         (space_id, node_id, actor_account_id, op_type, metadata) \
         VALUES ($1, $2, $3, 'metadata.patch', '{}'::jsonb) \
         RETURNING id, link_index_generation",
    )
    .bind(space_id)
    .bind(root_id)
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;
    let second_generation: i64 = sqlx::query_scalar(
        "INSERT INTO file_change_events \
         (id, space_id, node_id, actor_account_id, op_type, metadata) \
         VALUES ($1, $2, $3, $4, 'metadata.patch', '{}'::jsonb) \
         RETURNING link_index_generation",
    )
    .bind(lower_id)
    .bind(space_id)
    .bind(root_id)
    .bind(owner)
    .fetch_one(&db.pool)
    .await?;

    assert!(lower_id < higher_id);
    assert_eq!((first_generation, second_generation), (1, 2));
    let runs = drain(&service).await?;
    assert!(matches!(
        runs.as_slice(),
        [LinkIndexRun::Incremental { events: 2, .. }]
    ));
    let applied_generation: i64 = sqlx::query_scalar(
        "SELECT applied_generation FROM space_link_index_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(applied_generation, 2);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn missing_pending_generation_falls_back_to_a_rebuild()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-generation-gap",
        "link-index-generation-gap@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-generation-gap").await;
    let service = LinkIndexService::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );
    drain(&service).await?;

    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO file_change_events \
             (space_id, node_id, actor_account_id, op_type, metadata) \
             VALUES ($1, $2, $3, 'metadata.patch', '{}'::jsonb)",
        )
        .bind(space_id)
        .bind(root_id)
        .bind(owner)
        .execute(&db.pool)
        .await?;
    }
    sqlx::query(
        "DELETE FROM file_change_events \
         WHERE space_id = $1 AND link_index_generation = 1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert!(matches!(
        service.process_next().await?,
        LinkIndexRun::RebuildQueued { space_id: queued_space } if queued_space == space_id
    ));
    assert!(matches!(
        drain(&service).await?.as_slice(),
        [LinkIndexRun::Rebuilt { .. }]
    ));
    let (desired_generation, applied_generation): (i64, i64) = sqlx::query_as(
        "SELECT desired_generation, applied_generation \
         FROM space_link_index_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!((desired_generation, applied_generation), (2, 2));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn indexes_a_long_unresolved_href_without_a_btree_key_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-long-href",
        "link-index-long-href@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-long-href").await;
    let files = FilesRepo::new(db.pool.clone());
    let service = LinkIndexService::new(LinkIndexRepo::new(db.pool.clone()), files.clone());
    let href = format!("{}.md", "x".repeat(4_000));
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text(&format!("[missing]({href})")),
            owner,
        )
        .await?;

    drain(&service).await?;
    let links = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(links.outgoing_count, 1);
    assert_eq!(links.outgoing[0].raw_href, href);
    assert_eq!(links.outgoing[0].status, LinkReferenceStatus::Missing);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn an_older_worker_does_not_downgrade_a_newer_parser_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-newer-parser",
        "link-index-newer-parser@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, _root_id) = setup_space(&spaces, owner, "link-index-newer-parser").await;
    let service = LinkIndexService::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );

    drain(&service).await?;
    sqlx::query(
        "UPDATE space_link_index_states \
         SET parser_version = 2, status = 'ready', rebuild_requested = false \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert!(drain(&service).await?.is_empty());
    assert!(matches!(
        service.ensure_worker_compatible().await,
        Err(notegate_service::ServiceError::Internal(message))
            if message.contains("roll forward")
    ));
    let parser_version: i32 = sqlx::query_scalar(
        "SELECT parser_version FROM space_link_index_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(parser_version, 2);

    db.cleanup().await;
    Ok(())
}
