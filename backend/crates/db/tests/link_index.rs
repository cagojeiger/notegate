//! Integration tests for eventually consistent link index persistence.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::{TestDb, space_with_root};
use notegate_db::{
    FilesRepo, LINK_IMPACT_JOB_KIND, LINK_PARSER_VERSION, LINK_SOURCE_JOB_KIND,
    LINK_SPACE_JOB_KIND, LinkExpansion, LinkIndexRepo, LinkSourceCommit, LinkSourceDiscard,
    LinkSourceJob, LinkSourcePayload, LinkSpaceJob, LinkSpacePayload, MetadataMutationKind,
    NewLinkReference, StoredLinkReference, TextMutationKind,
};
use notegate_jobs::{ClaimedJob, JobQueue, JobSpec, NewJob};
use notegate_model::LinkReferenceKind;
use notegate_model::files::{CreateFolder, MoveNode, StoredContent, UpdateNode, WriteTextBody};
use serde_json::json;

fn text(content: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: format!("{:064x}", content.len()),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

async fn active_jobs(db: &TestDb, kind: &str, space_id: uuid::Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM background_jobs \
         WHERE job_kind = $1 AND payload ->> 'space_id' = $2::text \
           AND status IN ('queued', 'running')",
    )
    .bind(kind)
    .bind(space_id)
    .fetch_one(&db.pool)
    .await
}

async fn active_source_ids(
    db: &TestDb,
    space_id: uuid::Uuid,
) -> Result<BTreeSet<uuid::Uuid>, sqlx::Error> {
    let ids = sqlx::query_scalar(
        "SELECT (payload ->> 'source_node_id')::uuid FROM background_jobs \
         WHERE job_kind = 'node_link_source' \
           AND payload ->> 'space_id' = $1::text \
           AND status IN ('queued', 'running')",
    )
    .bind(space_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(ids.into_iter().collect())
}

async fn request_impact(
    db: &TestDb,
    space_id: uuid::Uuid,
    changed_node_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT enqueue_node_link_impact($1, $2)")
        .bind(space_id)
        .bind(changed_node_id)
        .fetch_one(&db.pool)
        .await
}

async fn insert_terminal_link_job(
    db: &TestDb,
    kind: &str,
    payload: serde_json::Value,
    status: &str,
    seconds_after_now: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO background_jobs ( \
             job_kind, payload, status, attempt_count, failure_count, max_attempts, \
             completed_at, created_at, updated_at \
         ) VALUES ( \
             $1, $2, $3, CASE WHEN $3 = 'dead' THEN 8 ELSE 1 END, \
             CASE WHEN $3 = 'dead' THEN 8 ELSE 0 END, 8, \
             now() + $4 * interval '1 second', \
             now() + $4 * interval '1 second', \
             now() + $4 * interval '1 second' \
         )",
    )
    .bind(kind)
    .bind(payload)
    .bind(status)
    .bind(seconds_after_now)
    .execute(&db.pool)
    .await?;
    Ok(())
}

async fn wait_until_space_is_locked(
    db: &TestDb,
    space_id: uuid::Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    for _attempt in 0..50 {
        let mut probe = db.pool.begin().await?;
        let result = sqlx::query(
            "SELECT id FROM spaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE NOWAIT",
        )
        .bind(space_id)
        .fetch_optional(&mut *probe)
        .await;
        let locked = match result {
            Ok(_) => false,
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03") => true,
            Err(error) => return Err(error.into()),
        };
        probe.rollback().await?;
        if locked {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err("link projection did not acquire the space lock".into())
}

async fn claim_job<J: JobSpec>(
    db: &TestDb,
    payload: J::Payload,
) -> Result<ClaimedJob, Box<dyn std::error::Error>> {
    let queue = JobQueue::new(db.pool.clone());
    queue.enqueue(&NewJob::<J>::new(payload)).await?;
    claim_existing_job(db, J::KIND).await
}

async fn claim_existing_job(
    db: &TestDb,
    kind: &str,
) -> Result<ClaimedJob, Box<dyn std::error::Error>> {
    let queue = JobQueue::new(db.pool.clone());
    queue
        .claim_many(
            "link-index-test",
            &[kind.to_owned()],
            Duration::from_secs(30),
            1,
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "expected link job claim".into())
}

#[tokio::test]
async fn new_space_queues_one_initial_full_rebuild() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "link-index-new-space").await?;

    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 1);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn empty_space_rebuild_records_latest_index_update() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_, space_id, _) = space_with_root(&db.pool, "link-index-empty-space").await?;
    let links = LinkIndexRepo::new(db.pool.clone());
    let queue = JobQueue::new(db.pool.clone());
    let job = claim_existing_job(&db, LINK_SPACE_JOB_KIND).await?;

    assert_eq!(
        links.expand_space(&job.fence(), space_id).await?,
        LinkExpansion::Expanded
    );
    assert!(queue.succeed(&job).await?);

    let status = links.space_status(space_id).await?;
    assert_eq!(status.outdated_documents, 0);
    assert!(status.latest_index_update_at.is_some());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn nonempty_space_records_an_update_only_after_a_source_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-nonempty-space").await?;
    FilesRepo::new(db.pool.clone())
        .insert_text(space_id, root_id, "note.md", &text("note"), account_id)
        .await?;
    let links = LinkIndexRepo::new(db.pool.clone());
    let queue = JobQueue::new(db.pool.clone());
    let job = claim_existing_job(&db, LINK_SPACE_JOB_KIND).await?;

    assert_eq!(
        links.expand_space(&job.fence(), space_id).await?,
        LinkExpansion::Expanded
    );
    assert!(queue.succeed(&job).await?);

    let status = links.space_status(space_id).await?;
    assert_eq!(status.outdated_documents, 1);
    assert_eq!(status.active_documents, 1);
    assert!(status.latest_index_update_at.is_none());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn file_changes_enqueue_only_the_required_scope() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-scope").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space_id, root_id, "note.md", &text("before"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    files
        .save_text_content(
            space_id,
            node.id,
            &text("after content"),
            None,
            account_id,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 1);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 0);
    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 0);

    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    sqlx::query(
        "INSERT INTO file_change_events ( \
             space_id, node_id, actor_account_id, op_type, metadata \
         ) VALUES ( \
             $1, $2, $3, 'item.update', \
             '{\"name_changed\":false,\"text_encryption_changed\":true}'::jsonb \
         )",
    )
    .bind(space_id)
    .bind(node.id)
    .bind(account_id)
    .execute(&db.pool)
    .await?;
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 0);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 0);

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
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 0);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 0);
    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 0);

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
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 1);
    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn fresh_link_requests_coalesce_without_losing_a_running_follow_up()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-coalesce").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let links = LinkIndexRepo::new(db.pool.clone());
    let (first, second) = tokio::join!(
        links.request_source(space_id, source.id),
        links.request_source(space_id, source.id),
    );
    assert!(first?);
    assert!(second?);
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 1);

    let queue = JobQueue::new(db.pool.clone());
    let claimed = queue
        .claim_many(
            "link-index-coalesce",
            &[LINK_SOURCE_JOB_KIND.to_owned()],
            Duration::from_secs(30),
            1,
        )
        .await?;
    assert_eq!(claimed.len(), 1);

    let (third, fourth) = tokio::join!(
        links.request_source(space_id, source.id),
        links.request_source(space_id, source.id),
    );
    assert!(third?);
    assert!(fourth?);
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 2);

    let (first_space, second_space) =
        tokio::join!(links.request_space(space_id), links.request_space(space_id),);
    assert!(first_space?);
    assert!(second_space?);
    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 1);

    let (first_impact, second_impact) = tokio::join!(
        request_impact(&db, space_id, source.id),
        request_impact(&db, space_id, source.id),
    );
    assert!(first_impact?);
    assert!(second_impact?);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 1);

    let _running_impact = claim_existing_job(&db, LINK_IMPACT_JOB_KIND).await?;
    let (third_impact, fourth_impact) = tokio::join!(
        request_impact(&db, space_id, source.id),
        request_impact(&db, space_id, source.id),
    );
    assert!(third_impact?);
    assert!(fourth_impact?);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 2);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn link_job_requests_do_not_wait_for_the_space_write_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-enqueue-lock").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let mut locked_space = db.pool.begin().await?;
    sqlx::query("SELECT id FROM spaces WHERE id = $1 FOR UPDATE")
        .bind(space_id)
        .fetch_one(&mut *locked_space)
        .await?;

    let links = LinkIndexRepo::new(db.pool.clone());
    tokio::time::timeout(Duration::from_secs(1), async {
        assert!(links.request_source(space_id, source.id).await?);
        assert!(request_impact(&db, space_id, source.id).await?);
        assert!(links.request_space(space_id).await?);
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await??;
    locked_space.rollback().await?;

    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 1);
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 1);
    assert_eq!(active_jobs(&db, LINK_SPACE_JOB_KIND, space_id).await?, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn impact_expansion_enqueues_only_sources_affected_by_the_changed_subtree()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-impact").await?;
    let files = FilesRepo::new(db.pool.clone());
    let folder = files
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "folder".to_owned(),
            },
            account_id,
        )
        .await?;
    let (target, _) = files
        .insert_text(
            space_id,
            folder.id,
            "target.md",
            &text("target"),
            account_id,
        )
        .await?;
    let (nested_source, _) = files
        .insert_text(
            space_id,
            folder.id,
            "nested.md",
            &text("nested"),
            account_id,
        )
        .await?;
    let (linked_source, _) = files
        .insert_text(space_id, root_id, "linked.md", &text("linked"), account_id)
        .await?;
    let (broken_source, _) = files
        .insert_text(space_id, root_id, "broken.md", &text("broken"), account_id)
        .await?;
    let (unrelated_source, _) = files
        .insert_text(
            space_id,
            root_id,
            "unrelated.md",
            &text("unrelated"),
            account_id,
        )
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_refs ( \
             space_id, source_node_id, target_node_id, target_path, \
             reference_kind, occurrence_count \
         ) VALUES \
             ($1, $2, $3, '/folder/target.md', 'link', 1), \
             ($1, $4, NULL, '/archive/target.md', 'link', 1)",
    )
    .bind(space_id)
    .bind(linked_source.id)
    .bind(target.id)
    .bind(broken_source.id)
    .execute(&db.pool)
    .await?;

    files
        .move_node(
            space_id,
            &MoveNode {
                node_id: folder.id,
                new_parent_node_id: root_id,
                new_name: Some("archive".to_owned()),
                expected_parent_id: Some(root_id),
            },
            account_id,
        )
        .await?;
    assert_eq!(active_jobs(&db, LINK_IMPACT_JOB_KIND, space_id).await?, 1);

    let impact = claim_existing_job(&db, LINK_IMPACT_JOB_KIND).await?;
    assert_eq!(
        LinkIndexRepo::new(db.pool.clone())
            .expand_impact(&impact.fence(), space_id, folder.id)
            .await?,
        LinkExpansion::Expanded
    );

    let expected = BTreeSet::from([
        target.id,
        nested_source.id,
        linked_source.id,
        broken_source.id,
    ]);
    assert_eq!(active_source_ids(&db, space_id).await?, expected);
    assert!(
        !active_source_ids(&db, space_id)
            .await?
            .contains(&unrelated_source.id)
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deleted_source_with_a_projection_is_enqueued_for_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-delete-source").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_source_states ( \
             space_id, source_node_id, source_content_sha256, source_path, parser_version \
         ) VALUES ($1, $2, $3, '/source.md', $4)",
    )
    .bind(space_id)
    .bind(source.id)
    .bind(&source_text.content_sha256)
    .bind(LINK_PARSER_VERSION)
    .execute(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO node_link_refs ( \
             space_id, source_node_id, target_node_id, target_path, \
             reference_kind, occurrence_count \
         ) VALUES ($1, $2, $3, '/target.md', 'link', 1)",
    )
    .bind(space_id)
    .bind(source.id)
    .bind(target.id)
    .execute(&db.pool)
    .await?;

    files
        .soft_delete_node(space_id, source.id, account_id, false)
        .await?;
    let impact = claim_existing_job(&db, LINK_IMPACT_JOB_KIND).await?;
    assert_eq!(
        links
            .expand_impact(&impact.fence(), space_id, source.id)
            .await?,
        LinkExpansion::Expanded
    );
    assert_eq!(
        active_source_ids(&db, space_id).await?,
        BTreeSet::from([source.id])
    );

    let cleanup = claim_existing_job(&db, LINK_SOURCE_JOB_KIND).await?;
    assert_eq!(
        links
            .discard_source(&cleanup.fence(), space_id, source.id)
            .await?,
        LinkSourceDiscard::Deleted
    );
    let remaining: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM node_link_refs \
                 WHERE space_id = $1 AND source_node_id = $2) \
              + (SELECT count(*) FROM node_link_source_states \
                 WHERE space_id = $1 AND source_node_id = $2)",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(remaining, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expanding_a_space_enqueues_live_sources_and_tombstone_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-expand").await?;
    let files = FilesRepo::new(db.pool.clone());
    files
        .insert_text(space_id, root_id, "one.md", &text("one"), account_id)
        .await?;
    let (deleted_source, deleted_text) = files
        .insert_text(space_id, root_id, "two.md", &text("two"), account_id)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_source_states ( \
             space_id, source_node_id, source_content_sha256, source_path, parser_version \
         ) VALUES ($1, $2, $3, '/two.md', $4)",
    )
    .bind(space_id)
    .bind(deleted_source.id)
    .bind(deleted_text.content_sha256)
    .bind(LINK_PARSER_VERSION)
    .execute(&db.pool)
    .await?;
    files
        .soft_delete_node(space_id, deleted_source.id, account_id, false)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    let links = LinkIndexRepo::new(db.pool.clone());
    let claim = claim_job::<LinkSpaceJob>(&db, LinkSpacePayload { space_id }).await?;
    assert_eq!(
        links.expand_space(&claim.fence(), space_id).await?,
        LinkExpansion::Expanded
    );
    assert_eq!(active_jobs(&db, LINK_SOURCE_JOB_KIND, space_id).await?, 2);
    let state_still_owned_by_source_job: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_source_states \
             WHERE space_id = $1 AND source_node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(deleted_source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(state_still_owned_by_source_job);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn outdated_parser_state_keeps_the_document_pending() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-parser-version").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_source_states ( \
             space_id, source_node_id, source_content_sha256, source_path, parser_version \
         ) VALUES ($1, $2, $3, '/source.md', $4)",
    )
    .bind(space_id)
    .bind(source.id)
    .bind(source_text.content_sha256)
    .bind(LINK_PARSER_VERSION + 1)
    .execute(&db.pool)
    .await?;

    let status = LinkIndexRepo::new(db.pool.clone())
        .space_status(space_id)
        .await?;
    assert_eq!(status.outdated_documents, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn purging_a_dead_source_job_does_not_hide_a_stale_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-purged-dead-source").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (source, original) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("original"),
            account_id,
        )
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_source_states ( \
             space_id, source_node_id, source_content_sha256, source_path, \
             parser_version, projected_at \
         ) VALUES ($1, $2, $3, '/source.md', $4, now() - interval '1 day')",
    )
    .bind(space_id)
    .bind(source.id)
    .bind(original.content_sha256)
    .bind(LINK_PARSER_VERSION)
    .execute(&db.pool)
    .await?;

    files
        .save_text_content(
            space_id,
            source.id,
            &text("changed"),
            None,
            account_id,
            TextMutationKind::Write,
        )
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    insert_terminal_link_job(
        &db,
        LINK_SOURCE_JOB_KIND,
        json!({ "space_id": space_id, "source_node_id": source.id }),
        "dead",
        -1,
    )
    .await?;

    let links = LinkIndexRepo::new(db.pool.clone());
    let failed = links.space_status(space_id).await?;
    assert_eq!(failed.failed_documents, 1);
    assert_eq!(failed.retrying_documents, 0);

    assert!(links.request_space(space_id).await?);
    let recovering = links.space_status(space_id).await?;
    assert!(recovering.space_pending);
    assert_eq!(recovering.failed_documents, 0);
    sqlx::query(
        "DELETE FROM background_jobs \
         WHERE job_kind = 'node_link_space' \
           AND payload ->> 'space_id' = $1::text \
           AND status = 'queued'",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    assert_eq!(
        JobQueue::new(db.pool.clone())
            .purge_completed(Duration::ZERO, 100)
            .await?,
        1
    );
    let after_purge = links.space_status(space_id).await?;
    assert_eq!(after_purge.failed_documents, 1);
    assert_eq!(after_purge.outdated_documents, 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn space_status_uses_projection_health_instead_of_terminal_job_history()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-status-domain-truth").await?;
    let (source, source_text) = FilesRepo::new(db.pool.clone())
        .insert_text(space_id, root_id, "source.md", &text("current"), account_id)
        .await?;
    sqlx::query("DELETE FROM background_jobs WHERE payload ->> 'space_id' = $1::text")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    sqlx::query(
        "INSERT INTO node_link_source_states ( \
             space_id, source_node_id, source_content_sha256, source_path, parser_version \
         ) VALUES ($1, $2, $3, '/source.md', $4)",
    )
    .bind(space_id)
    .bind(source.id)
    .bind(source_text.content_sha256)
    .bind(LINK_PARSER_VERSION)
    .execute(&db.pool)
    .await?;
    sqlx::query("INSERT INTO node_link_space_states (space_id) VALUES ($1)")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    insert_terminal_link_job(
        &db,
        LINK_SOURCE_JOB_KIND,
        json!({ "space_id": space_id, "source_node_id": source.id }),
        "dead",
        1,
    )
    .await?;
    insert_terminal_link_job(
        &db,
        LINK_IMPACT_JOB_KIND,
        json!({ "space_id": space_id, "changed_node_id": root_id }),
        "dead",
        2,
    )
    .await?;
    let links = LinkIndexRepo::new(db.pool.clone());
    let status = links.space_status(space_id).await?;
    assert_eq!(status.outdated_documents, 0);
    assert_eq!(status.failed_documents, 0);
    assert!(!status.space_failed);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn source_replacement_is_atomic_and_rejects_stale_content()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-replace").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), account_id)
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let claim = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;
    let initial = NewLinkReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    };
    let stored = StoredLinkReference {
        target_node_id: Some(target.id),
        target_path: initial.target_path.clone(),
        kind: initial.kind,
        occurrence_count: initial.occurrence_count,
    };

    assert_eq!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                std::slice::from_ref(&initial),
            )
            .await?,
        LinkSourceCommit::Applied
    );
    let invalid = NewLinkReference {
        occurrence_count: 0,
        ..initial.clone()
    };
    assert!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                &[invalid],
            )
            .await
            .is_err()
    );
    assert_eq!(
        links.outgoing(space_id, source.id, 100, None).await?,
        vec![stored.clone()]
    );

    let changed = text("changed source content");
    files
        .save_text_content(
            space_id,
            source.id,
            &changed,
            None,
            account_id,
            TextMutationKind::Write,
        )
        .await?;
    assert_eq!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                &[],
            )
            .await?,
        LinkSourceCommit::Stale
    );
    assert_eq!(
        links.outgoing(space_id, source.id, 100, None).await?,
        vec![stored]
    );
    assert_eq!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &changed.content_sha256,
                &source_path,
                &[],
            )
            .await?,
        LinkSourceCommit::Applied
    );
    assert!(
        links
            .outgoing(space_id, source.id, 100, None)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn source_path_change_fences_an_old_projection() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-path").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let old_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("old path");
    let claim = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;
    files
        .update_node(
            space_id,
            &UpdateNode {
                node_id: source.id,
                name: Some("renamed.md".to_owned()),
                sort_order: None,
            },
            account_id,
        )
        .await?;

    assert_eq!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &old_path,
                &[],
            )
            .await?,
        LinkSourceCommit::Stale
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn target_paths_are_resolved_from_the_final_space_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-target-snapshot").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), account_id)
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let claim = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;

    files
        .update_node(
            space_id,
            &UpdateNode {
                node_id: target.id,
                name: Some("renamed.md".to_owned()),
                sort_order: None,
            },
            account_id,
        )
        .await?;
    assert_eq!(
        links
            .complete_source(
                &claim.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                &[NewLinkReference {
                    target_path: "/target.md".to_owned(),
                    kind: LinkReferenceKind::Link,
                    occurrence_count: 1,
                }],
            )
            .await?,
        LinkSourceCommit::Applied
    );
    assert_eq!(
        links.outgoing(space_id, source.id, 100, None).await?,
        vec![StoredLinkReference {
            target_node_id: None,
            target_path: "/target.md".to_owned(),
            kind: LinkReferenceKind::Link,
            occurrence_count: 1,
        }]
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_claim_cannot_replace_link_relationships() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) =
        space_with_root(&db.pool, "link-index-claim-fence").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let queue = JobQueue::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let (target, _) = files
        .insert_text(space_id, root_id, "target.md", &text("target"), account_id)
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let reference = NewLinkReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    };
    let stored = StoredLinkReference {
        target_node_id: Some(target.id),
        target_path: reference.target_path.clone(),
        kind: reference.kind,
        occurrence_count: reference.occurrence_count,
    };
    let initial = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;
    assert_eq!(
        links
            .complete_source(
                &initial.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                std::slice::from_ref(&reference),
            )
            .await?,
        LinkSourceCommit::Applied
    );
    assert!(queue.succeed(&initial).await?);

    let expired = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(expired.job_id)
    .execute(&db.pool)
    .await?;
    assert_eq!(
        links
            .complete_source(
                &expired.fence(),
                space_id,
                source.id,
                &source_text.content_sha256,
                &source_path,
                &[],
            )
            .await?,
        LinkSourceCommit::ClaimLost
    );
    assert_eq!(queue.recover_expired(10).await?.retried, 1);
    assert_eq!(
        links.outgoing(space_id, source.id, 100, None).await?,
        vec![stored]
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn link_projection_does_not_block_claim_heartbeats_during_domain_work()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, space_id, root_id) = space_with_root(&db.pool, "link-index-heartbeat").await?;
    let files = FilesRepo::new(db.pool.clone());
    let links = LinkIndexRepo::new(db.pool.clone());
    let queue = JobQueue::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(space_id, root_id, "source.md", &text("source"), account_id)
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let claim = claim_job::<LinkSourceJob>(
        &db,
        LinkSourcePayload {
            space_id,
            source_node_id: source.id,
        },
    )
    .await?;

    let mut blocker = db.pool.begin().await?;
    sqlx::query("SELECT id FROM nodes WHERE id = $1 AND space_id = $2 FOR UPDATE")
        .bind(source.id)
        .bind(space_id)
        .fetch_one(&mut *blocker)
        .await?;

    let projection = tokio::spawn({
        let links = links.clone();
        let fence = claim.fence();
        let expected_content_sha256 = source_text.content_sha256.clone();
        let expected_path = source_path.clone();
        async move {
            links
                .complete_source(
                    &fence,
                    space_id,
                    source.id,
                    &expected_content_sha256,
                    &expected_path,
                    &[],
                )
                .await
        }
    });
    wait_until_space_is_locked(&db, space_id).await?;

    let heartbeat = tokio::time::timeout(
        Duration::from_secs(1),
        queue.heartbeat(&claim, Duration::from_secs(30)),
    )
    .await??;
    assert!(heartbeat);

    blocker.rollback().await?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), projection).await???,
        LinkSourceCommit::Applied
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn link_targets_must_belong_to_the_same_space() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account_id, first_space_id, first_root_id) =
        space_with_root(&db.pool, "link-index-first-space").await?;
    let (_, second_space_id, second_root_id) =
        space_with_root(&db.pool, "link-index-second-space").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            first_space_id,
            first_root_id,
            "source.md",
            &text("source"),
            account_id,
        )
        .await?;
    let (same_space_target, _) = files
        .insert_text(
            first_space_id,
            first_root_id,
            "same-space-target.md",
            &text("target"),
            account_id,
        )
        .await?;
    let (other_space_target, _) = files
        .insert_text(
            second_space_id,
            second_root_id,
            "target.md",
            &text("target"),
            account_id,
        )
        .await?;

    let result = sqlx::query(
        "INSERT INTO node_link_refs ( \
            space_id, source_node_id, target_node_id, target_path, \
            reference_kind, occurrence_count \
         ) VALUES ($1, $2, $3, '/target.md', 'link', 1)",
    )
    .bind(first_space_id)
    .bind(source.id)
    .bind(other_space_target.id)
    .execute(&db.pool)
    .await;
    let error = result.expect_err("cross-space target must violate the foreign key");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database_error| database_error.code())
            .as_deref(),
        Some("23503")
    );

    sqlx::query(
        "INSERT INTO node_link_refs ( \
            space_id, source_node_id, target_node_id, target_path, \
            reference_kind, occurrence_count \
         ) VALUES ($1, $2, $3, '/same-space-target.md', 'link', 1)",
    )
    .bind(first_space_id)
    .bind(source.id)
    .bind(same_space_target.id)
    .execute(&db.pool)
    .await?;
    sqlx::query("DELETE FROM nodes WHERE id = $1 AND space_id = $2")
        .bind(same_space_target.id)
        .bind(first_space_id)
        .execute(&db.pool)
        .await?;
    let stored_target_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_node_id FROM node_link_refs \
         WHERE space_id = $1 AND source_node_id = $2",
    )
    .bind(first_space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored_target_id, None);

    db.cleanup().await;
    Ok(())
}
