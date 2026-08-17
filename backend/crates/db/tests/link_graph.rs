#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use std::time::Duration;

use common::{TestDb, space_with_root};
use notegate_db::{
    FilesRepo, LinkGraphChangeCollection, LinkGraphProjectNodesJob, LinkGraphProjection,
    LinkGraphProjectionClaim, LinkGraphRepo, LinkGraphSourceSnapshot, LinkGraphStoredReference,
    LinkGraphWorkRepo, SpaceRepo, TextMutationKind,
};
use notegate_jobs::{ClaimedJob, JobQueue, JobSpec};
use notegate_model::files::{CreateFolder, StoredContent, WriteTextBody};
use notegate_model::{LinkReferenceKind, NodeLinkGraphStatus};
use uuid::Uuid;

fn text(content: &str, hash_character: char) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(content.to_owned()),
        content_sha256: hash_character.to_string().repeat(64),
        byte_len: content.len() as i64,
        line_count: content.lines().count().max(1) as i32,
    }
}

async fn projection_claim(
    pool: &sqlx::PgPool,
    work: &LinkGraphWorkRepo,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<LinkGraphProjectionClaim, Box<dyn std::error::Error>> {
    let (job, request_version) = claim_projection_job(pool, work, space_id, node_id).await?;
    Ok(LinkGraphProjectionClaim {
        fence: job.fence(),
        request_version,
    })
}

async fn claim_projection_job(
    pool: &sqlx::PgPool,
    work: &LinkGraphWorkRepo,
    space_id: Uuid,
    node_id: Uuid,
) -> Result<(ClaimedJob, i64), Box<dyn std::error::Error>> {
    work.request_nodes(space_id, &[node_id]).await?;
    let mut jobs = JobQueue::new(pool.clone())
        .claim_many(
            &format!("link-graph-test-{}", Uuid::new_v4()),
            &[LinkGraphProjectNodesJob::KIND.to_owned()],
            Duration::from_secs(300),
            1,
        )
        .await?;
    let job = jobs.pop().expect("projection job");
    let request_version: i64 = sqlx::query_scalar(
        "SELECT active_request_version \
         FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2 AND active_job_id = $3",
    )
    .bind(space_id)
    .bind(node_id)
    .bind(job.job_id)
    .fetch_one(pool)
    .await?;
    Ok((job, request_version))
}

async fn clear_projection_work(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("DELETE FROM node_link_projection_targets")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM background_jobs")
        .execute(pool)
        .await?;
    Ok(())
}

async fn collect_due(
    pool: &sqlx::PgPool,
    work: &LinkGraphWorkRepo,
) -> Result<LinkGraphChangeCollection, Box<dyn std::error::Error>> {
    sqlx::query(
        "UPDATE space_change_processor_states SET available_at = now() \
         WHERE processor_kind = 'link_graph' AND processing_state = 'pending'",
    )
    .execute(pool)
    .await?;
    Ok(work.collect_changes().await?)
}

async fn insert_text_nodes(
    pool: &sqlx::PgPool,
    account_id: Uuid,
    space_id: Uuid,
    root_id: Uuid,
    name_prefix: &str,
    count: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query(
        "WITH created AS ( \
             INSERT INTO nodes ( \
                 space_id, parent_id, name, kind, \
                 created_by_account_id, updated_by_account_id \
             ) \
             SELECT $1, $2, format('%s-%s.md', $4, item), 'text', $3, $3 \
             FROM generate_series(1, $5) AS generated(item) \
             RETURNING id \
         ) \
         INSERT INTO text_objects ( \
             node_id, space_id, storage_format, content_text, content_sha256, \
             byte_len, line_count, created_by_account_id, updated_by_account_id \
         ) \
         SELECT id, $1, 'plain', '', repeat('a', 64), 0, 0, $3, $3 \
         FROM created",
    )
    .bind(space_id)
    .bind(root_id)
    .bind(account_id)
    .bind(name_prefix)
    .bind(count)
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn stale_source_snapshot_cannot_replace_a_newer_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-stale").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, old_text) = files
        .insert_text(space_id, root_id, "source.md", &text("old", 'a'), account)
        .await?;
    let (target, _) = files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", 'b'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let (_, current_text) = files
        .save_text_content(
            space_id,
            source.id,
            &text("new", 'c'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    let references = vec![LinkGraphStoredReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];

    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                projection_claim(&db.pool, &work, space_id, source.id).await?,
                LinkGraphSourceSnapshot {
                    content_sha256: &old_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Stale
    );
    let pending_target: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(!pending_target);
    assert_eq!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Idle
    );
    assert!(
        graph
            .outgoing(space_id, source.id, 10, None)
            .await?
            .is_empty()
    );
    assert_eq!(
        graph
            .fail_projection_target(
                space_id,
                source.id,
                projection_claim(&db.pool, &work, space_id, source.id).await?,
                "link_reference_limit_exceeded",
                &old_text.content_sha256,
                &source_path,
            )
            .await?,
        LinkGraphProjection::Stale
    );
    let failed_target_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(!failed_target_exists);

    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                projection_claim(&db.pool, &work, space_id, source.id).await?,
                LinkGraphSourceSnapshot {
                    content_sha256: &current_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );
    assert_eq!(
        graph.outgoing(space_id, source.id, 10, None).await?[0].target_node_id,
        Some(target.id)
    );

    files
        .soft_delete_node(space_id, target.id, account, false)
        .await?;
    assert_eq!(
        graph
            .reconcile_non_text_node(
                space_id,
                target.id,
                projection_claim(&db.pool, &work, space_id, target.id).await?,
            )
            .await?,
        LinkGraphProjection::Removed
    );
    let outgoing = graph.outgoing(space_id, source.id, 10, None).await?;
    assert_eq!(outgoing[0].target_path, "/target.md");
    assert_eq!(outgoing[0].target_node_id, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn projection_waiting_for_source_does_not_lock_its_target()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-lock-order").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](./target.md)", 'd'),
            account,
        )
        .await?;
    files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", 'e'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let source_id = source.id;
    let claim = projection_claim(&db.pool, &work, space_id, source_id).await?;
    let references = vec![LinkGraphStoredReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];

    let mut writer = db.pool.begin().await?;
    sqlx::query(
        "SELECT 1 FROM nodes node \
         JOIN text_objects text ON text.node_id = node.id AND text.space_id = node.space_id \
         WHERE node.space_id = $1 AND node.id = $2 FOR UPDATE OF node, text",
    )
    .bind(space_id)
    .bind(source_id)
    .execute(&mut *writer)
    .await?;

    let projection_graph = graph.clone();
    let source_hash = source_text.content_sha256.clone();
    let projection = tokio::spawn(async move {
        projection_graph
            .replace_source(
                space_id,
                source_id,
                claim,
                LinkGraphSourceSnapshot {
                    content_sha256: &source_hash,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await
    });

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting_for_source: bool = sqlx::query_scalar(
                "SELECT EXISTS ( \
                     SELECT 1 FROM pg_stat_activity activity \
                     JOIN pg_locks relation_lock ON relation_lock.pid = activity.pid \
                     WHERE activity.datname = current_database() \
                       AND activity.wait_event_type = 'Lock' \
                       AND activity.query LIKE '%FOR NO KEY UPDATE OF node, text%' \
                       AND relation_lock.relation = 'nodes'::regclass \
                       AND relation_lock.mode = 'RowShareLock' AND relation_lock.granted \
                 )",
            )
            .fetch_one(&db.pool)
            .await?;
            if waiting_for_source {
                return Ok::<(), sqlx::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;

    let mut target_probe = db.pool.begin().await?;
    sqlx::query(
        "SELECT 1 FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2 FOR UPDATE NOWAIT",
    )
    .bind(space_id)
    .bind(source_id)
    .execute(&mut *target_probe)
    .await?;
    target_probe.commit().await?;

    writer.commit().await?;
    let projection_result = tokio::time::timeout(Duration::from_secs(5), projection).await??;
    assert_eq!(
        projection_result?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn mutually_linked_sources_project_without_deadlocking()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-mutual-locks").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (first, first_text) = files
        .insert_text(
            space_id,
            root_id,
            "first.md",
            &text("[second](./second.md)", '6'),
            account,
        )
        .await?;
    let (second, second_text) = files
        .insert_text(
            space_id,
            root_id,
            "second.md",
            &text("[first](./first.md)", '7'),
            account,
        )
        .await?;
    let first_path = files
        .node_path(space_id, first.id)
        .await?
        .expect("first path");
    let second_path = files
        .node_path(space_id, second.id)
        .await?
        .expect("second path");
    let first_claim = projection_claim(&db.pool, &work, space_id, first.id).await?;
    let second_claim = projection_claim(&db.pool, &work, space_id, second.id).await?;
    let first_references = [LinkGraphStoredReference {
        target_path: second_path.clone(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];
    let second_references = [LinkGraphStoredReference {
        target_path: first_path.clone(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];

    let first_projection = graph.replace_source(
        space_id,
        first.id,
        first_claim,
        LinkGraphSourceSnapshot {
            content_sha256: &first_text.content_sha256,
            path: &first_path,
            parser_version: 1,
            references: &first_references,
        },
    );
    let second_projection = graph.replace_source(
        space_id,
        second.id,
        second_claim,
        LinkGraphSourceSnapshot {
            content_sha256: &second_text.content_sha256,
            path: &second_path,
            parser_version: 1,
            references: &second_references,
        },
    );
    let (first_result, second_result) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(first_projection, second_projection)
    })
    .await?;
    assert_eq!(
        first_result?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );
    assert_eq!(
        second_result?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn change_collection_does_not_wait_for_a_source_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-collector-writer").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", '9'),
            account,
        )
        .await?;
    sqlx::query(
        "UPDATE space_change_processor_states SET available_at = now() \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    let mut writer = db.pool.begin().await?;
    sqlx::query("SELECT 1 FROM nodes WHERE space_id = $1 AND id = $2 FOR UPDATE")
        .bind(space_id)
        .bind(source.id)
        .execute(&mut *writer)
        .await?;

    let collected = tokio::time::timeout(Duration::from_secs(5), work.collect_changes()).await??;
    assert!(matches!(
        collected,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            staged_targets: 1,
            dispatched_targets: 1,
            jobs: 1,
            ..
        }
    ));

    writer.rollback().await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deleted_space_projection_leaves_space_cleanup_to_the_collector()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-delete-lock-order").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", '0'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let claim = projection_claim(&db.pool, &work, space_id, source.id).await?;

    SpaceRepo::new(db.pool.clone())
        .delete_space(space_id, account, account)
        .await?;
    let mut collector = db.pool.begin().await?;
    sqlx::query(
        "SELECT 1 FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph' FOR UPDATE",
    )
    .bind(space_id)
    .execute(&mut *collector)
    .await?;

    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(5),
            graph.replace_source(
                space_id,
                source.id,
                claim,
                LinkGraphSourceSnapshot {
                    content_sha256: &source_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &[],
                },
            ),
        )
        .await??,
        LinkGraphProjection::Removed
    );
    collector.commit().await?;

    assert!(matches!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 0,
            failed_targets: 0,
            dispatched_targets: 0,
            jobs: 0,
            has_more: false,
        }
    ));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn change_events_wait_for_and_extend_the_quiet_period()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-debounce").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(space_id, root_id, "source.md", &text("first", '1'), account)
        .await?;

    let first_available_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT available_at FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    let first_is_delayed: bool =
        sqlx::query_scalar("SELECT $1 > clock_timestamp() + interval '4 minutes 50 seconds'")
            .bind(first_available_at)
            .fetch_one(&db.pool)
            .await?;
    assert!(first_is_delayed);
    assert_eq!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Idle
    );

    tokio::time::sleep(Duration::from_millis(10)).await;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("second", '2'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    let second_available_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT available_at FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(second_available_at > first_available_at);
    assert_eq!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Idle
    );

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            staged_targets: 1,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
            ..
        }
    ));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn incremental_rebuild_does_not_pull_new_events_across_its_quiet_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-incremental-window").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'a'),
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    clear_projection_work(&db.pool).await?;

    sqlx::query(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         SELECT $1, $2, 'text.write', '{}'::jsonb FROM generate_series(1, 500)",
    )
    .bind(space_id)
    .bind(source.id)
    .execute(&db.pool)
    .await?;
    let event_window_id: i64 = sqlx::query_scalar(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         VALUES ($1, $2, 'item.move', '{}'::jsonb) RETURNING id",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            events: 500,
            has_more: true,
            ..
        }
    ));
    let stored_window_id: Option<i64> = sqlx::query_scalar(
        "SELECT incremental_event_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(stored_window_id, Some(event_window_id));

    let next_event_id: i64 = sqlx::query_scalar(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         VALUES ($1, $2, 'text.write', '{}'::jsonb) RETURNING id",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(matches!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Collected {
            events: 1,
            has_more: false,
            ..
        }
    ));
    let (checkpoint, continue_immediately, incremental_event_id, waits_for_quiet): (
        i64,
        bool,
        Option<i64>,
        bool,
    ) = sqlx::query_as(
        "SELECT last_processed_event_id, continue_immediately, incremental_event_id, \
                available_at > now() \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(checkpoint, event_window_id);
    assert!(!continue_immediately);
    assert_eq!(incremental_event_id, None);
    assert!(waits_for_quiet);
    assert_eq!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Idle
    );

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            events: 1,
            has_more: false,
            ..
        }
    ));
    let final_checkpoint: i64 = sqlx::query_scalar(
        "SELECT last_processed_event_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(final_checkpoint, next_event_id);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn change_collector_coalesces_content_changes_and_rebuilds_after_delete()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-collector").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());

    assert_eq!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Idle
    );

    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("initial", 'd'),
            account,
        )
        .await?;
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 1,
            failed_targets: 0,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
        }
    ));
    clear_projection_work(&db.pool).await?;

    files
        .save_text_content(
            space_id,
            source.id,
            &text("first", 'e'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("second", 'f'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 2,
            staged_targets: 1,
            failed_targets: 0,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
        }
    ));
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(payload["space_id"], space_id.to_string());
    assert_eq!(payload["node_ids"][0], source.id.to_string());

    let folder = files
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "folder".to_owned(),
            },
            account,
        )
        .await?;
    let (_child, _) = files
        .insert_text(
            space_id,
            folder.id,
            "child.md",
            &text("child", '1'),
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    clear_projection_work(&db.pool).await?;

    files
        .soft_delete_node(space_id, folder.id, account, true)
        .await?;
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 1,
            staged_targets: 1,
            failed_targets: 0,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
        }
    ));
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes'",
    )
    .fetch_one(&db.pool)
    .await?;
    let projected_ids = payload["node_ids"]
        .as_array()
        .expect("node ids")
        .iter()
        .map(|value| Uuid::parse_str(value.as_str().expect("node id")).expect("uuid"))
        .collect::<Vec<_>>();
    assert_eq!(projected_ids, vec![source.id]);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn delete_change_stages_a_full_scan_in_bounded_passes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-delete-bounded").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());

    let folder = files
        .insert_folder(
            space_id,
            &CreateFolder {
                parent_node_id: root_id,
                name: "deleted".to_owned(),
            },
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    clear_projection_work(&db.pool).await?;

    insert_text_nodes(&db.pool, account, space_id, root_id, "live", 501).await?;
    files
        .soft_delete_node(space_id, folder.id, account, false)
        .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 1,
            staged_targets: 500,
            failed_targets: 0,
            dispatched_targets: 500,
            jobs: 10,
            has_more: true,
        }
    ));
    let target_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_link_projection_targets WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(target_count, 500);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn change_collector_processes_pending_spaces_without_scanning_idle_spaces()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (first_account, first_space, first_root) =
        space_with_root(&db.pool, "link-pending-first").await?;
    let (second_account, second_space, second_root) =
        space_with_root(&db.pool, "link-pending-second").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());

    files
        .insert_text(
            first_space,
            first_root,
            "first.md",
            &text("first", '2'),
            first_account,
        )
        .await?;
    files
        .insert_text(
            second_space,
            second_root,
            "second.md",
            &text("second", '3'),
            second_account,
        )
        .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 2,
            events: 0,
            staged_targets: 2,
            failed_targets: 0,
            dispatched_targets: 2,
            jobs: 2,
            has_more: false,
        }
    ));
    assert_eq!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Idle
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn first_collection_full_scans_when_initial_events_were_pruned()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-initial-scan").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'f'),
            account,
        )
        .await?;

    sqlx::query("DELETE FROM file_change_events WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;
    let (state, requires_full_scan): (String, bool) = sqlx::query_as(
        "SELECT processing_state, requires_full_scan \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(state, "pending");
    assert!(requires_full_scan);

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 1,
            failed_targets: 0,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
        }
    ));
    let staged: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(staged);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn a_pruned_first_pending_event_falls_back_to_a_full_scan()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-pruned-first-event").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    sqlx::query(
        "INSERT INTO space_change_processor_states ( \
             space_id, processor_kind, processing_state, available_at, requires_full_scan \
         ) VALUES ($1, 'link_graph', 'idle', NULL, false)",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", '8'),
            account,
        )
        .await?;
    let pending_since_event_id: Option<i64> = sqlx::query_scalar(
        "SELECT pending_since_event_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(pending_since_event_id.is_some());

    sqlx::query("DELETE FROM file_change_events WHERE space_id = $1")
        .bind(space_id)
        .execute(&db.pool)
        .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 1,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
            ..
        }
    ));
    let staged: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(staged);

    clear_projection_work(&db.pool).await?;
    files
        .save_text_content(
            space_id,
            source.id,
            &text("checkpoint", '9'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    clear_projection_work(&db.pool).await?;
    let checkpoint: i64 = sqlx::query_scalar(
        "SELECT last_processed_event_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(checkpoint > 0);

    files
        .save_text_content(
            space_id,
            source.id,
            &text("pending", 'a'),
            None,
            account,
            TextMutationKind::Write,
        )
        .await?;
    let pending_event_id: i64 = sqlx::query_scalar(
        "SELECT pending_since_event_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(pending_event_id > checkpoint);
    sqlx::query("DELETE FROM file_change_events WHERE id = $1")
        .bind(pending_event_id)
        .execute(&db.pool)
        .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 1,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
            ..
        }
    ));

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn a_full_scan_absorbs_the_remaining_event_backlog() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-full-scan-backlog").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'b'),
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    clear_projection_work(&db.pool).await?;

    sqlx::query(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         SELECT $1, $2, 'folder.create', '{}'::jsonb FROM generate_series(1, 501)",
    )
    .bind(space_id)
    .bind(source.id)
    .execute(&db.pool)
    .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 500,
            staged_targets: 1,
            failed_targets: 0,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
        }
    ));
    assert_eq!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Idle
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn change_events_mark_each_registered_processor_independently()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) =
        space_with_root(&db.pool, "link-processor-isolation").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    sqlx::query(
        "INSERT INTO space_change_processor_states (space_id, processor_kind) VALUES ($1, $2)",
    )
    .bind(space_id)
    .bind("ai_analysis")
    .execute(&db.pool)
    .await?;

    files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", '4'),
            account,
        )
        .await?;

    let pending_before: Vec<String> = sqlx::query_scalar(
        "SELECT processor_kind FROM space_change_processor_states \
         WHERE space_id = $1 AND processing_state = 'pending' ORDER BY processor_kind",
    )
    .bind(space_id)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(pending_before, vec!["ai_analysis", "link_graph"]);

    collect_due(&db.pool, &work).await?;

    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT processor_kind, processing_state FROM space_change_processor_states \
         WHERE space_id = $1 ORDER BY processor_kind",
    )
    .bind(space_id)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(
        states,
        vec![
            ("ai_analysis".to_owned(), "pending".to_owned()),
            ("link_graph".to_owned(), "idle".to_owned()),
        ]
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn event_committed_after_an_idle_transition_restores_pending_state()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_account, space_id, _root_id) = space_with_root(&db.pool, "link-wakeup").await?;
    sqlx::query(
        "INSERT INTO space_change_processor_states ( \
             space_id, processor_kind, processing_state, available_at, requires_full_scan \
         ) VALUES ($1, 'link_graph', 'pending', now(), false) \
         ON CONFLICT (space_id, processor_kind) DO UPDATE \
         SET processing_state = 'pending', available_at = now(), requires_full_scan = false",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    let mut collector = db.pool.begin().await?;
    sqlx::query(
        "SELECT 1 FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph' FOR UPDATE",
    )
    .bind(space_id)
    .execute(&mut *collector)
    .await?;
    let writer_pool = db.pool.clone();
    let writer = tokio::spawn(async move {
        sqlx::query(
            "INSERT INTO file_change_events (space_id, op_type, metadata) \
             VALUES ($1, 'text.write', '{}'::jsonb)",
        )
        .bind(space_id)
        .execute(&writer_pool)
        .await
    });

    sqlx::query(
        "UPDATE space_change_processor_states \
         SET processing_state = 'idle', available_at = NULL, \
             pending_since_event_id = NULL, requires_full_scan = false, \
             full_scan_event_id = NULL, \
             full_scan_after_node_id = NULL \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .execute(&mut *collector)
    .await?;
    collector.commit().await?;
    writer.await??;

    let state: String = sqlx::query_scalar(
        "SELECT processing_state FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(state, "pending");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn dead_projection_job_is_recorded_and_manual_sync_reactivates_the_node()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-redrive").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", '5'),
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;

    let first_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    let max_attempts: i32 =
        sqlx::query_scalar("SELECT max_attempts FROM background_jobs WHERE job_id = $1")
            .bind(first_job_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(max_attempts, 8);
    mark_job_dead(&db.pool, first_job_id).await?;
    let terminal_state = LinkGraphRepo::new(db.pool.clone())
        .state(space_id, source.id)
        .await?;
    assert_eq!(terminal_state.status, NodeLinkGraphStatus::Failed);
    assert_eq!(
        terminal_state.failure_code.as_deref(),
        Some("link_graph_projection_failed")
    );
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 0,
            events: 0,
            staged_targets: 0,
            failed_targets: 1,
            dispatched_targets: 0,
            jobs: 0,
            has_more: false,
        }
    ));
    let failed: (
        Option<Uuid>,
        Option<i64>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT active_job_id, active_request_version, failure_code, failed_at \
             FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(failed.0, None);
    assert_eq!(failed.1, None);
    assert_eq!(failed.2.as_deref(), Some("link_graph_projection_failed"));
    assert!(failed.3.is_some());
    assert_eq!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Idle
    );
    let job_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(job_count, 1);

    work.request_nodes(space_id, &[source.id]).await?;
    let reactivated: (
        i64,
        Uuid,
        i64,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT request_version, active_job_id, active_request_version, \
                    failure_code, failed_at \
             FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_ne!(reactivated.1, first_job_id);
    assert_eq!(reactivated.0, reactivated.2);
    assert_eq!(reactivated.3, None);
    assert_eq!(reactivated.4, None);
    let reactivated_state = LinkGraphRepo::new(db.pool.clone())
        .state(space_id, source.id)
        .await?;
    assert_eq!(reactivated_state.status, NodeLinkGraphStatus::Syncing);
    assert_eq!(reactivated_state.failure_code, None);

    mark_job_dead(&db.pool, reactivated.1).await?;
    collect_due(&db.pool, &work).await?;
    assert!(work.request_space(space_id).await?);
    let full_reindex_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_ne!(full_reindex_job_id, reactivated.1);
    let full_reindex_state = LinkGraphRepo::new(db.pool.clone())
        .state(space_id, source.id)
        .await?;
    assert_eq!(full_reindex_state.status, NodeLinkGraphStatus::Syncing);
    assert_eq!(full_reindex_state.failure_code, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn pending_space_changes_mask_an_older_text_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-pending-state").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'p'),
            account,
        )
        .await?;
    sqlx::query(
        "INSERT INTO node_link_projection_targets ( \
             space_id, node_id, failure_code, failed_at \
         ) VALUES ($1, $2, 'previous_failure', now())",
    )
    .bind(space_id)
    .bind(source.id)
    .execute(&db.pool)
    .await?;

    let pending = graph.state(space_id, source.id).await?;
    assert_eq!(pending.status, NodeLinkGraphStatus::Pending);
    assert_eq!(pending.failure_code, None);
    assert_eq!(pending.failed_at, None);
    assert_eq!(
        graph.state(space_id, root_id).await?.status,
        NodeLinkGraphStatus::Idle
    );

    sqlx::query(
        "UPDATE space_change_processor_states \
         SET processing_state = 'idle', available_at = NULL, \
             pending_since_event_id = NULL, requires_full_scan = false, \
             full_scan_event_id = NULL, \
             full_scan_after_node_id = NULL \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;
    let failed = graph.state(space_id, source.id).await?;
    assert_eq!(failed.status, NodeLinkGraphStatus::Failed);
    assert_eq!(failed.failure_code.as_deref(), Some("previous_failure"));
    assert!(failed.failed_at.is_some());

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn manual_sync_supersedes_an_unsettled_dead_projection_job()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-dead-superseded").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'd'),
            account,
        )
        .await?;
    collect_due(&db.pool, &work).await?;
    let first_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    mark_job_dead(&db.pool, first_job_id).await?;

    work.request_nodes(space_id, &[source.id]).await?;

    let (request_version, active_job_id, active_request_version, failure_code): (
        i64,
        Uuid,
        i64,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT request_version, active_job_id, active_request_version, failure_code \
         FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_ne!(active_job_id, first_job_id);
    assert_eq!(active_request_version, request_version);
    assert_eq!(failure_code, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn duplicate_node_requests_coalesce_before_job_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-duplicate-request").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'a'),
            account,
        )
        .await?;

    work.request_nodes(space_id, &[source.id, source.id])
        .await?;
    let target_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM background_jobs \
         WHERE job_kind = 'link_graph_project_nodes'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(target_count, 1);
    assert_eq!(payload["node_ids"].as_array().expect("node ids").len(), 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn collected_events_for_already_purged_nodes_are_ignored()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (_account, space_id, _root_id) = space_with_root(&db.pool, "link-purged-event").await?;
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    sqlx::query(
        "INSERT INTO space_change_processor_states ( \
             space_id, processor_kind, processing_state, available_at, requires_full_scan \
         ) VALUES ($1, 'link_graph', 'idle', NULL, false)",
    )
    .bind(space_id)
    .execute(&db.pool)
    .await?;

    sqlx::query(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         VALUES ($1, $2, 'text.write', '{}'::jsonb)",
    )
    .bind(space_id)
    .bind(Uuid::new_v4())
    .execute(&db.pool)
    .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 1,
            staged_targets: 0,
            failed_targets: 0,
            dispatched_targets: 0,
            jobs: 0,
            has_more: false,
        }
    ));
    let targets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_link_projection_targets WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(targets, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_reindex_repairs_relation_sources_without_projection_state()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-repair-source").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'b'),
            account,
        )
        .await?;
    let (target, _) = files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", 'c'),
            account,
        )
        .await?;
    clear_projection_work(&db.pool).await?;
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
        .soft_delete_node(space_id, source.id, account, false)
        .await?;

    assert!(work.request_space(space_id).await?);
    let staged: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2 \
         )",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(staged);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_reindex_reactivates_work_for_a_hard_deleted_node()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-repair-orphan-work").await?;
    let files = FilesRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, _) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("source", 'd'),
            account,
        )
        .await?;
    work.request_nodes(space_id, &[source.id]).await?;
    let first_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    mark_job_dead(&db.pool, first_job_id).await?;
    sqlx::query("DELETE FROM nodes WHERE space_id = $1 AND id = $2")
        .bind(space_id)
        .bind(source.id)
        .execute(&db.pool)
        .await?;

    assert!(work.request_space(space_id).await?);
    let next_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_ne!(next_job_id, first_job_id);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn a_new_request_version_survives_completion_of_an_older_job()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-version-fence").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](./target.md)", '6'),
            account,
        )
        .await?;
    files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", '7'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let old_claim = projection_claim(&db.pool, &work, space_id, source.id).await?;

    work.request_nodes(space_id, &[source.id]).await?;
    let (newer_version, newer_job_id): (i64, Uuid) = sqlx::query_as(
        "SELECT request_version, active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert!(newer_version > old_claim.request_version);

    let references = [LinkGraphStoredReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];
    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                old_claim,
                LinkGraphSourceSnapshot {
                    content_sha256: &source_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Stale
    );
    let (preserved_version, active_job_id): (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT request_version, active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(preserved_version, newer_version);
    assert_eq!(active_job_id, Some(newer_job_id));
    assert!(
        graph
            .outgoing(space_id, source.id, 10, None)
            .await?
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn deleting_a_space_wakes_collection_and_removes_derived_graph_data()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-space-delete").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](./target.md)", '8'),
            account,
        )
        .await?;
    files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", '9'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let references = [LinkGraphStoredReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];
    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                projection_claim(&db.pool, &work, space_id, source.id).await?,
                LinkGraphSourceSnapshot {
                    content_sha256: &source_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );

    SpaceRepo::new(db.pool.clone())
        .delete_space(space_id, account, account)
        .await?;
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 0,
            failed_targets: 0,
            dispatched_targets: 0,
            jobs: 0,
            has_more: false,
        }
    ));
    let graph_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM node_link_refs WHERE space_id = $1")
            .bind(space_id)
            .fetch_one(&db.pool)
            .await?;
    let processor_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM space_change_processor_states WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(graph_rows, 0);
    assert_eq!(processor_rows, 0);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn full_reindex_stages_and_dispatches_in_bounded_passes_without_losing_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-full-dispatch").await?;
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let node_count = 1_001_i64;
    insert_text_nodes(&db.pool, account, space_id, root_id, "doc", node_count).await?;

    assert!(work.request_space(space_id).await?);

    let (initial_job_count, initial_queued_nodes): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(jsonb_array_length(payload -> 'node_ids')), 0) \
         FROM background_jobs WHERE job_kind = $1",
    )
    .bind(LinkGraphProjectNodesJob::KIND)
    .fetch_one(&db.pool)
    .await?;
    let (target_count, initial_ready_count): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER ( \
             WHERE active_job_id IS NULL AND failed_at IS NULL \
         ) \
         FROM node_link_projection_targets WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(initial_job_count, 10);
    assert_eq!(initial_queued_nodes, 500);
    assert_eq!(target_count, 500);
    assert_eq!(initial_ready_count, 0);

    let (scan_boundary, scan_cursor): (i64, Uuid) = sqlx::query_as(
        "SELECT full_scan_event_id, full_scan_after_node_id \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph' \
           AND requires_full_scan",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    let changed_node_id: Uuid = sqlx::query_scalar(
        "SELECT node_id FROM node_link_projection_targets \
         WHERE space_id = $1 ORDER BY node_id LIMIT 1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         VALUES ($1, $2, 'text.write', '{}'::jsonb)",
    )
    .bind(space_id)
    .bind(changed_node_id)
    .execute(&db.pool)
    .await?;

    assert!(matches!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 500,
            dispatched_targets: 500,
            jobs: 10,
            has_more: true,
            ..
        }
    ));
    let (mid_target_count, mid_job_count): (i64, i64) = sqlx::query_as(
        "SELECT \
             (SELECT count(*) FROM node_link_projection_targets WHERE space_id = $1), \
             (SELECT count(*) FROM background_jobs WHERE job_kind = $2)",
    )
    .bind(space_id)
    .bind(LinkGraphProjectNodesJob::KIND)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(mid_target_count, 1_000);
    assert_eq!(mid_job_count, 20);

    assert!(matches!(
        work.collect_changes().await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 0,
            staged_targets: 1,
            dispatched_targets: 1,
            jobs: 1,
            has_more: false,
            ..
        }
    ));
    let (requires_full_scan, pending, checkpoint, continue_immediately, debounce_pending): (
        bool,
        bool,
        i64,
        bool,
        bool,
    ) = sqlx::query_as(
        "SELECT requires_full_scan, processing_state = 'pending', last_processed_event_id, \
                continue_immediately, available_at > now() \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(!requires_full_scan);
    assert!(pending);
    assert_eq!(checkpoint, scan_boundary);
    assert!(!continue_immediately);
    assert!(debounce_pending);

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            spaces: 1,
            events: 1,
            staged_targets: 1,
            dispatched_targets: 0,
            jobs: 0,
            has_more: false,
            ..
        }
    ));
    let (job_count, queued_nodes): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(jsonb_array_length(payload -> 'node_ids')), 0) \
         FROM background_jobs WHERE job_kind = $1",
    )
    .bind(LinkGraphProjectNodesJob::KIND)
    .fetch_one(&db.pool)
    .await?;
    let ready_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM node_link_projection_targets \
         WHERE space_id = $1 AND active_job_id IS NULL AND failed_at IS NULL",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(job_count, 21);
    assert_eq!(queued_nodes, node_count);
    assert_eq!(ready_count, 0);
    let final_state: (String, bool, Option<i64>, Option<Uuid>, i64) = sqlx::query_as(
        "SELECT processing_state, requires_full_scan, full_scan_event_id, \
                full_scan_after_node_id, last_processed_event_id \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(final_state.0, "idle");
    assert!(!final_state.1);
    assert_eq!(final_state.2, None);
    assert_eq!(final_state.3, None);
    assert!(final_state.4 > scan_boundary);
    assert!(changed_node_id <= scan_cursor);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn manual_reindex_restarts_an_active_full_scan_from_a_new_event_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-full-restart").await?;
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    insert_text_nodes(&db.pool, account, space_id, root_id, "restart", 1_001).await?;

    assert!(work.request_space(space_id).await?);
    let (first_boundary, first_cursor): (i64, Uuid) = sqlx::query_as(
        "SELECT full_scan_event_id, full_scan_after_node_id \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected { has_more: true, .. }
    ));
    let middle_cursor: Uuid = sqlx::query_scalar(
        "SELECT full_scan_after_node_id FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(middle_cursor > first_cursor);

    let changed_node_id: Uuid = sqlx::query_scalar(
        "SELECT node_id FROM node_link_projection_targets \
         WHERE space_id = $1 ORDER BY node_id LIMIT 1",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "INSERT INTO file_change_events (space_id, node_id, op_type, metadata) \
         VALUES ($1, $2, 'text.write', '{}'::jsonb)",
    )
    .bind(space_id)
    .bind(changed_node_id)
    .execute(&db.pool)
    .await?;

    assert!(work.request_space(space_id).await?);
    let (restarted_boundary, restarted_cursor): (i64, Uuid) = sqlx::query_as(
        "SELECT full_scan_event_id, full_scan_after_node_id \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert!(restarted_boundary > first_boundary);
    assert_eq!(restarted_cursor, first_cursor);

    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected { has_more: true, .. }
    ));
    assert!(matches!(
        collect_due(&db.pool, &work).await?,
        LinkGraphChangeCollection::Collected {
            has_more: false,
            ..
        }
    ));
    let final_state: (String, bool, i64, Option<i64>, Option<Uuid>) = sqlx::query_as(
        "SELECT processing_state, requires_full_scan, last_processed_event_id, \
                full_scan_event_id, full_scan_after_node_id \
         FROM space_change_processor_states \
         WHERE space_id = $1 AND processor_kind = 'link_graph'",
    )
    .bind(space_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(final_state.0, "idle");
    assert!(!final_state.1);
    assert_eq!(final_state.2, restarted_boundary);
    assert_eq!(final_state.3, None);
    assert_eq!(final_state.4, None);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn expired_claim_cannot_publish_after_the_job_is_reclaimed()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (account, space_id, root_id) = space_with_root(&db.pool, "link-claim-fence").await?;
    let files = FilesRepo::new(db.pool.clone());
    let graph = LinkGraphRepo::new(db.pool.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let (source, source_text) = files
        .insert_text(
            space_id,
            root_id,
            "source.md",
            &text("[target](./target.md)", 'a'),
            account,
        )
        .await?;
    files
        .insert_text(
            space_id,
            root_id,
            "target.md",
            &text("target", 'b'),
            account,
        )
        .await?;
    let source_path = files
        .node_path(space_id, source.id)
        .await?
        .expect("source path");
    let (expired_job, request_version) =
        claim_projection_job(&db.pool, &work, space_id, source.id).await?;
    sqlx::query(
        "UPDATE background_jobs SET lease_until = now() - interval '1 second' \
         WHERE job_id = $1",
    )
    .bind(expired_job.job_id)
    .execute(&db.pool)
    .await?;
    let queue = JobQueue::new(db.pool.clone());
    let recovery = queue.recover_expired(1).await?;
    assert_eq!(recovery.retried, 1);
    let mut reclaimed = queue
        .claim_many(
            "link-graph-replacement",
            &[LinkGraphProjectNodesJob::KIND.to_owned()],
            Duration::from_secs(300),
            1,
        )
        .await?;
    let replacement_job = reclaimed.pop().expect("replacement projection job");
    assert_eq!(replacement_job.job_id, expired_job.job_id);
    assert_ne!(replacement_job.claim_token, expired_job.claim_token);

    let references = [LinkGraphStoredReference {
        target_path: "/target.md".to_owned(),
        kind: LinkReferenceKind::Link,
        occurrence_count: 1,
    }];
    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                LinkGraphProjectionClaim {
                    fence: expired_job.fence(),
                    request_version,
                },
                LinkGraphSourceSnapshot {
                    content_sha256: &source_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Stale
    );
    assert!(
        graph
            .outgoing(space_id, source.id, 10, None)
            .await?
            .is_empty()
    );
    assert_eq!(
        graph
            .replace_source(
                space_id,
                source.id,
                LinkGraphProjectionClaim {
                    fence: replacement_job.fence(),
                    request_version,
                },
                LinkGraphSourceSnapshot {
                    content_sha256: &source_text.content_sha256,
                    path: &source_path,
                    parser_version: 1,
                    references: &references,
                },
            )
            .await?,
        LinkGraphProjection::Applied { reference_count: 1 }
    );

    db.cleanup().await;
    Ok(())
}

async fn mark_job_dead(
    pool: &sqlx::PgPool,
    job_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query(
        "UPDATE background_jobs \
         SET status = 'dead', attempt_count = max_attempts, failure_count = 1, \
             claim_token = NULL, claimed_by = NULL, lease_until = NULL, \
             last_error_code = 'link_graph_projection_failed', \
             last_error_message = 'projection failed', \
             completed_at = now(), updated_at = now() \
         WHERE job_id = $1",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}
