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
use notegate_model::files::StoredContent;
use notegate_model::{AccountKind, LinkIndexFreshness, LinkIndexStatus, LinkReferenceStatus};
use notegate_service::files::{FilesService, UpdateTextEncryption, content};
use notegate_service::link_index::{LinkIndexProjector, LinkIndexRun, LinkIndexService};

const PARSER_VERSION: i32 = 1;

fn text(content: &str) -> StoredContent {
    content::compute(content).into_stored_plain(content.to_owned())
}

async fn drain(
    projector: &LinkIndexProjector,
) -> Result<Vec<LinkIndexRun>, Box<dyn std::error::Error>> {
    let mut runs = Vec::new();
    for _ in 0..8 {
        match projector.process_next().await? {
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
    let service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index, files.clone());

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

    let runs = drain(&projector).await?;
    assert!(matches!(
        runs.as_slice(),
        [LinkIndexRun::Incremental { events: 2, .. }]
    ));

    let source_links = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(source_links.index.freshness(), LinkIndexFreshness::Current);
    assert_eq!(source_links.outgoing_count, 2);
    assert_eq!(source_links.broken_count, 1);
    assert_eq!(
        source_links.outgoing[0].source_path.as_deref(),
        Some("/source.md")
    );
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
    assert_eq!(
        target_links.incoming[0].target_path.as_deref(),
        Some("/target.md")
    );

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

    drain(&projector).await?;
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
    drain(&projector).await?;
    let deleted_target = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(deleted_target.broken_count, 1);
    assert_eq!(
        deleted_target.outgoing[0].status,
        LinkReferenceStatus::Deleted
    );

    let (replacement, _) = files
        .insert_text(space_id, root_id, "latest.md", &text("replacement"), owner)
        .await?;
    drain(&projector).await?;
    let rebound = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(rebound.broken_count, 0);
    assert_eq!(rebound.outgoing[0].status, LinkReferenceStatus::Resolved);
    assert_eq!(rebound.outgoing[0].target_node_id, Some(replacement.id));

    files
        .soft_delete_node(space_id, source.id, owner, false)
        .await?;
    drain(&projector).await?;
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
    index.request_rebuild(space_id).await?;

    let first = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
        .await?
        .expect("first worker claims queued space");
    assert_eq!(first.space_id, space_id);
    assert!(
        index
            .claim_next(Duration::from_secs(30), PARSER_VERSION)
            .await?
            .is_none()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn large_text_fanout_is_applied_in_bounded_incremental_runs()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-bounded-incremental",
        "link-index-bounded-incremental@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-bounded-incremental").await;
    let files = FilesRepo::new(db.pool.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index, files.clone());

    for number in 0..9 {
        files
            .insert_text(
                space_id,
                root_id,
                &format!("source-{number}.md"),
                &text("no links"),
                owner,
            )
            .await?;
    }

    let runs = drain(&projector).await?;
    assert!(matches!(
        runs.as_slice(),
        [
            LinkIndexRun::Incremental { events: 8, .. },
            LinkIndexRun::Incremental { events: 1, .. }
        ]
    ));
    assert_eq!(
        service.state(owner, space_id).await?.freshness(),
        LinkIndexFreshness::Current
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn server_encrypted_text_updates_persisted_link_relations()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-encrypted-source",
        "link-index-encrypted-source@example.test",
    )
    .await?;
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner)
        .execute(&db.pool)
        .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-encrypted-source").await;
    let files = FilesRepo::new(db.pool.clone());
    let files_service = FilesService::new(files.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let link_service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index, files.clone());

    let (first_target, _) = files
        .insert_text(space_id, root_id, "first.md", &text("first"), owner)
        .await?;
    let (second_target, _) = files
        .insert_text(space_id, root_id, "second.md", &text("second"), owner)
        .await?;
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[first](first.md)"),
            owner,
        )
        .await?;
    drain(&projector).await?;

    files_service
        .update_text_encryption(
            AccountKind::User,
            owner,
            space_id,
            UpdateTextEncryption {
                node_id: source.id,
                enabled: true,
            },
        )
        .await?;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("[second](second.md)"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    drain(&projector).await?;

    let source_links = link_service.node_links(owner, space_id, source.id).await?;
    assert_eq!(source_links.outgoing_count, 1);
    assert_eq!(source_links.outgoing[0].raw_href, "second.md");
    assert_eq!(
        source_links.outgoing[0].target_node_id,
        Some(second_target.id)
    );
    assert_eq!(
        link_service
            .node_links(owner, space_id, first_target.id)
            .await?
            .incoming_count,
        0
    );
    assert_eq!(
        link_service
            .node_links(owner, space_id, second_target.id)
            .await?
            .incoming_count,
        1
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rebuild_request_does_not_queue_a_second_active_rebuild()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-rebuild-idempotency",
        "link-index-rebuild-idempotency@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, _root_id) = setup_space(&spaces, owner, "link-index-rebuild-idempotency").await;
    let index = LinkIndexRepo::new(db.pool.clone());
    index.request_rebuild(space_id).await?;
    let claim = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
        .await?
        .expect("queued space is claimed");
    let base_generation = index
        .begin_rebuild(&claim, PARSER_VERSION, Duration::from_secs(30))
        .await?;

    let state = index.request_rebuild(space_id).await?;
    assert_eq!(state.status, LinkIndexStatus::Rebuilding);
    let rebuild_requested: bool = sqlx::query_scalar(
        "SELECT rebuild_requested FROM space_link_index_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(!rebuild_requested);

    index.finish_rebuild(&claim, base_generation).await?;
    assert!(
        index
            .claim_next(Duration::from_secs(30), PARSER_VERSION)
            .await?
            .is_none()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rebuild_request_resumes_a_failed_rebuild_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-rebuild-resume",
        "link-index-rebuild-resume@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, _root_id) = setup_space(&spaces, owner, "link-index-rebuild-resume").await;
    let index = LinkIndexRepo::new(db.pool.clone());
    index.request_rebuild(space_id).await?;
    let claim = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
        .await?
        .expect("queued space is claimed");
    let base_generation = index
        .begin_rebuild(&claim, PARSER_VERSION, Duration::from_secs(30))
        .await?;
    index.fail_claim(&claim, "retry the rebuild").await?;

    let state = index.request_rebuild(space_id).await?;
    assert_eq!(state.status, LinkIndexStatus::Rebuilding);
    let (saved_base, rebuild_requested, retry_count, ready_now): (Option<i64>, bool, i32, bool) =
        sqlx::query_as(
            "SELECT rebuild_base_generation, rebuild_requested, retry_count, run_after <= now() \
             FROM space_link_index_states WHERE space_id = $1",
        )
        .bind(space_id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(saved_base, Some(base_generation));
    assert!(!rebuild_requested);
    assert_eq!(retry_count, 0);
    assert!(ready_now);

    let resumed = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
        .await?
        .expect("failed rebuild is immediately reclaimable");
    assert_eq!(resumed.rebuild_base_generation, Some(base_generation));
    index.finish_rebuild(&resumed, base_generation).await?;

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn existing_space_waits_for_manual_initial_indexing() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-manual-initial",
        "link-index-manual-initial@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-manual-initial").await;
    let files = FilesRepo::new(db.pool.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index.clone(), files.clone());
    let (_target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), owner)
        .await?;
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](target.md)"),
            owner,
        )
        .await?;

    sqlx::query(
        "UPDATE space_link_index_states \
         SET status = 'uninitialized', rebuild_requested = false, applied_generation = 0 \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert!(matches!(
        projector.process_next().await?,
        LinkIndexRun::Idle
    ));
    let pending = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(pending.index.freshness(), LinkIndexFreshness::Uninitialized);
    assert_eq!(pending.outgoing_count, 0);
    assert!(pending.outgoing.is_empty());

    let state = index.request_rebuild(space_id).await?;
    assert_eq!(state.status, LinkIndexStatus::Rebuilding);
    let indexing = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(indexing.index.freshness(), LinkIndexFreshness::Rebuilding);
    assert_eq!(indexing.outgoing_count, 0);
    assert!(indexing.outgoing.is_empty());
    assert!(matches!(
        drain(&projector).await?.as_slice(),
        [LinkIndexRun::Rebuilt { .. }]
    ));
    let indexed = service.node_links(owner, space_id, source.id).await?;
    assert_eq!(indexed.index.freshness(), LinkIndexFreshness::Current);
    assert_eq!(indexed.outgoing_count, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn failed_resumable_rebuild_never_exposes_partial_relations()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-hidden-partial",
        "link-index-hidden-partial@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-hidden-partial").await;
    let files = FilesRepo::new(db.pool.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index.clone(), files.clone());
    for source_index in 0..9 {
        files
            .insert_text(
                space_id,
                root_id,
                &format!("source-{source_index}.md"),
                &text("[missing](target.md)"),
                owner,
            )
            .await?;
    }
    index.request_rebuild(space_id).await?;

    assert!(matches!(
        projector.process_next().await?,
        LinkIndexRun::RebuildProgress { sources: 8, .. }
    ));
    let partial_source_id: uuid::Uuid =
        sqlx::query_scalar("SELECT source_node_id FROM node_link_refs WHERE space_id = $1 LIMIT 1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    let claim = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
        .await?
        .expect("resumable rebuild is reclaimable");
    index.fail_claim(&claim, "simulated failure").await?;

    let failed = service
        .node_links(owner, space_id, partial_source_id)
        .await?;
    assert_eq!(failed.index.freshness(), LinkIndexFreshness::Failed);
    assert_eq!(failed.outgoing_count, 0);
    assert!(failed.outgoing.is_empty());

    let retrying = index.request_rebuild(space_id).await?;
    assert_eq!(retrying.status, LinkIndexStatus::Rebuilding);
    let rebuilding = service
        .node_links(owner, space_id, partial_source_id)
        .await?;
    assert_eq!(rebuilding.index.freshness(), LinkIndexFreshness::Rebuilding);
    assert_eq!(rebuilding.outgoing_count, 0);
    assert!(rebuilding.outgoing.is_empty());

    assert!(matches!(
        drain(&projector).await?.as_slice(),
        [LinkIndexRun::Rebuilt { .. }]
    ));
    let current = service
        .node_links(owner, space_id, partial_source_id)
        .await?;
    assert_eq!(current.index.freshness(), LinkIndexFreshness::Current);
    assert_eq!(current.outgoing_count, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn rebuild_releases_its_claim_after_each_source_batch()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-index-rebuild-batches",
        "link-index-rebuild-batches@example.test",
    )
    .await?;
    let spaces = SpaceRepo::new(db.pool.clone());
    let (space_id, root_id) = setup_space(&spaces, owner, "link-index-rebuild-batches").await;
    let files = FilesRepo::new(db.pool.clone());
    let index = LinkIndexRepo::new(db.pool.clone());
    let projector = LinkIndexProjector::new(index.clone(), files.clone());
    for index in 0..9 {
        files
            .insert_text(
                space_id,
                root_id,
                &format!("source-{index}.md"),
                &text("[missing](target.md)"),
                owner,
            )
            .await?;
    }
    index.request_rebuild(space_id).await?;

    assert!(matches!(
        projector.process_next().await?,
        LinkIndexRun::RebuildProgress {
            space_id: progress_space,
            sources: 8,
        } if progress_space == space_id
    ));
    let (status, claim_token, cursor): (String, Option<uuid::Uuid>, Option<uuid::Uuid>) =
        sqlx::query_as(
            "SELECT status, claim_token, rebuild_after_node_id \
             FROM space_link_index_states WHERE space_id = $1",
        )
        .bind(space_id)
        .fetch_one(&db.pool)
        .await?;
    assert_eq!(status, "rebuilding");
    assert!(claim_token.is_none());
    assert!(cursor.is_some());

    assert!(matches!(
        projector.process_next().await?,
        LinkIndexRun::Rebuilt { space_id: rebuilt_space } if rebuilt_space == space_id
    ));
    assert!(matches!(
        projector.process_next().await?,
        LinkIndexRun::Idle
    ));
    let reference_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_link_refs WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(reference_count, 9);

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
    index.request_rebuild(space_id).await?;

    let expired = index
        .claim_next(Duration::ZERO, PARSER_VERSION)
        .await?
        .expect("queued space is claimed");
    assert!(matches!(
        index
            .begin_rebuild(&expired, PARSER_VERSION, Duration::from_secs(30))
            .await,
        Err(notegate_core::Error::Conflict(_))
    ));
    let replacement = index
        .claim_next(Duration::from_secs(30), PARSER_VERSION)
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
    let projector = LinkIndexProjector::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );
    drain(&projector).await?;

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
    let runs = drain(&projector).await?;
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
    let projector = LinkIndexProjector::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );
    drain(&projector).await?;

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
        projector.process_next().await?,
        LinkIndexRun::RebuildQueued { space_id: queued_space } if queued_space == space_id
    ));
    assert!(matches!(
        drain(&projector).await?.as_slice(),
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
    let index = LinkIndexRepo::new(db.pool.clone());
    let service = LinkIndexService::new(index.clone(), files.clone());
    let projector = LinkIndexProjector::new(index, files.clone());
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

    drain(&projector).await?;
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
    let projector = LinkIndexProjector::new(
        LinkIndexRepo::new(db.pool.clone()),
        FilesRepo::new(db.pool.clone()),
    );

    drain(&projector).await?;
    sqlx::query(
        "UPDATE space_link_index_states \
         SET parser_version = 2, status = 'ready', rebuild_requested = false \
         WHERE space_id = $1",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert!(drain(&projector).await?.is_empty());
    assert!(matches!(
        projector.ensure_compatible().await,
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
