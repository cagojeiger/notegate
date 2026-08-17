#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use common::{TestDb, insert_user_account, setup_space};
use notegate_db::{
    FilesRepo, LinkGraphProjectNodesJob, LinkGraphRepo, LinkGraphWorkRepo, SpaceRepo,
};
use notegate_jobs::{JobQueue, JobSpec};
use notegate_model::{ListLinkReferences, files::DeleteNode};
use notegate_service::files::{
    CreateFolder, CreateText, FilesService, WriteTarget, WriteText, WriteTextBody,
};
use notegate_service::link_graph::{LinkGraphProjectionBatch, LinkGraphService};
use uuid::Uuid;

async fn project_requested_nodes(
    pool: &sqlx::PgPool,
    work: &LinkGraphWorkRepo,
    graph: &LinkGraphService,
    space_id: Uuid,
    node_ids: &[Uuid],
) -> Result<LinkGraphProjectionBatch, Box<dyn std::error::Error>> {
    work.request_nodes(space_id, node_ids).await?;
    let mut jobs = JobQueue::new(pool.clone())
        .claim_many(
            &format!("link-graph-service-test-{}", Uuid::new_v4()),
            &[LinkGraphProjectNodesJob::KIND.to_owned()],
            std::time::Duration::from_secs(300),
            1,
        )
        .await?;
    let job = jobs.pop().expect("projection job");
    Ok(graph.project_job(job.fence(), space_id, node_ids).await?)
}

#[tokio::test]
async fn projection_replaces_outgoing_and_derives_incoming_links()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "link-graph", "link-graph@example.test").await?;
    let (space_id, root_id) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "link-graph").await;
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let graph = LinkGraphService::new(
        LinkGraphRepo::new(db.pool.clone()),
        files_repo,
        work.clone(),
    );

    let docs = files
        .create_folder(
            owner,
            space_id,
            CreateFolder {
                parent_node_id: root_id,
                name: "docs".to_owned(),
            },
        )
        .await?;
    let source = files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: docs.node.id,
                name: "source.md".to_owned(),
            },
        )
        .await?;
    let target = files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "target.md".to_owned(),
            },
        )
        .await?;
    let source_id = source.node.node.id;
    let target_id = target.node.node.id;
    files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: source_id },
                body: WriteTextBody::Plain(
                    "[one](../target.md) [again](../target.md#top) [missing](../missing.md)"
                        .to_owned(),
                ),
                expected_sha256: None,
            },
        )
        .await?;

    let projected =
        project_requested_nodes(&db.pool, &work, &graph, space_id, &[source_id]).await?;
    assert_eq!(projected.projected, 1);
    assert_eq!(projected.stale, 0);

    let outgoing = graph
        .outgoing(owner, space_id, source_id, ListLinkReferences::default())
        .await?;
    assert_eq!(outgoing.items.len(), 2);
    let resolved = outgoing
        .items
        .iter()
        .find(|reference| reference.path == "/target.md")
        .expect("resolved reference");
    assert_eq!(resolved.node_id, Some(target_id));
    assert_eq!(resolved.occurrence_count, 2);
    let broken = outgoing
        .items
        .iter()
        .find(|reference| reference.path == "/missing.md")
        .expect("broken reference");
    assert_eq!(broken.node_id, None);

    let incoming = graph
        .incoming(owner, space_id, target_id, ListLinkReferences::default())
        .await?;
    assert_eq!(incoming.items.len(), 1);
    assert_eq!(incoming.items[0].node_id, Some(source_id));
    assert_eq!(incoming.items[0].path, "/docs/source.md");
    assert_eq!(incoming.items[0].occurrence_count, 2);
    assert!(
        graph
            .node_state(owner, space_id, source_id)
            .await?
            .projected_at
            .is_some()
    );

    files
        .delete_node(
            owner,
            space_id,
            DeleteNode {
                node_id: target_id,
                recursive: false,
            },
        )
        .await?;
    project_requested_nodes(&db.pool, &work, &graph, space_id, &[target_id]).await?;
    let outgoing = graph
        .outgoing(owner, space_id, source_id, ListLinkReferences::default())
        .await?;
    assert_eq!(
        outgoing
            .items
            .iter()
            .find(|reference| reference.path == "/target.md")
            .and_then(|reference| reference.node_id),
        None
    );

    let replacement = files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "target.md".to_owned(),
            },
        )
        .await?;
    project_requested_nodes(&db.pool, &work, &graph, space_id, &[source_id]).await?;
    let outgoing = graph
        .outgoing(owner, space_id, source_id, ListLinkReferences::default())
        .await?;
    assert_eq!(
        outgoing
            .items
            .iter()
            .find(|reference| reference.path == "/target.md")
            .and_then(|reference| reference.node_id),
        Some(replacement.node.node.id)
    );

    files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: source_id },
                body: WriteTextBody::Plain("no links".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    project_requested_nodes(&db.pool, &work, &graph, space_id, &[source_id]).await?;
    assert!(
        graph
            .outgoing(owner, space_id, source_id, ListLinkReferences::default())
            .await?
            .items
            .is_empty()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn client_encrypted_source_does_not_block_plain_sources_in_the_same_job()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner =
        insert_user_account(&db.pool, "link-encrypted", "link-encrypted@example.test").await?;
    let (space_id, root_id) =
        setup_space(&SpaceRepo::new(db.pool.clone()), owner, "link-encrypted").await;
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let graph = LinkGraphService::new(
        LinkGraphRepo::new(db.pool.clone()),
        files_repo,
        work.clone(),
    );

    let target = files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "target.md".to_owned(),
            },
        )
        .await?;
    let plain = files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root_id,
                    name: "plain.md".to_owned(),
                },
                body: WriteTextBody::Plain("[target](./target.md)".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    let encrypted = files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root_id,
                    name: "encrypted.md".to_owned(),
                },
                body: WriteTextBody::Encrypted(serde_json::json!({
                    "version": 1,
                    "ciphertext_b64": "opaque"
                })),
                expected_sha256: None,
            },
        )
        .await?;
    let node_ids = [plain.node.node.id, encrypted.node.node.id];

    let result = project_requested_nodes(&db.pool, &work, &graph, space_id, &node_ids).await?;
    assert_eq!(result.projected, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.stale, 0);
    assert_eq!(
        graph
            .outgoing(
                owner,
                space_id,
                plain.node.node.id,
                ListLinkReferences::default(),
            )
            .await?
            .items[0]
            .node_id,
        Some(target.node.node.id)
    );
    assert!(
        graph
            .node_state(owner, space_id, encrypted.node.node.id)
            .await?
            .projected_at
            .is_none()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn oversized_source_fails_without_blocking_other_sources_in_the_same_job()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-reference-limit",
        "link-reference-limit@example.test",
    )
    .await?;
    let (space_id, root_id) = setup_space(
        &SpaceRepo::new(db.pool.clone()),
        owner,
        "link-reference-limit",
    )
    .await;
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let graph = LinkGraphService::new(
        LinkGraphRepo::new(db.pool.clone()),
        files_repo,
        work.clone(),
    );
    files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "target.md".to_owned(),
            },
        )
        .await?;
    let valid = files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root_id,
                    name: "valid.md".to_owned(),
                },
                body: WriteTextBody::Plain("[target](./target.md)".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    let oversized_content = (0..=notegate_core::limits::LINK_REFERENCES_PER_TEXT_MAX)
        .map(|index| format!("[{index}](./target-{index}.md)"))
        .collect::<Vec<_>>()
        .join(" ");
    let oversized = files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root_id,
                    name: "oversized.md".to_owned(),
                },
                body: WriteTextBody::Plain(oversized_content),
                expected_sha256: None,
            },
        )
        .await?;
    let node_ids = [valid.node.node.id, oversized.node.node.id];

    let result = project_requested_nodes(&db.pool, &work, &graph, space_id, &node_ids).await?;

    assert_eq!(result.projected, 1);
    assert_eq!(result.failed, 1);
    assert_eq!(
        graph
            .outgoing(
                owner,
                space_id,
                valid.node.node.id,
                ListLinkReferences::default(),
            )
            .await?
            .items
            .len(),
        1
    );
    let failure_code: Option<String> = sqlx::query_scalar(
        "SELECT failure_code FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(oversized.node.node.id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(
        failure_code.as_deref(),
        Some("link_reference_limit_exceeded")
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn client_encryption_removes_a_previous_plain_text_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-encrypted-transition",
        "link-encrypted-transition@example.test",
    )
    .await?;
    let (space_id, root_id) = setup_space(
        &SpaceRepo::new(db.pool.clone()),
        owner,
        "link-encrypted-transition",
    )
    .await;
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let graph = LinkGraphService::new(
        LinkGraphRepo::new(db.pool.clone()),
        files_repo,
        work.clone(),
    );
    files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "target.md".to_owned(),
            },
        )
        .await?;
    let source = files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Create {
                    parent_node_id: root_id,
                    name: "source.md".to_owned(),
                },
                body: WriteTextBody::Plain("[target](./target.md)".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    let source_id = source.node.node.id;
    project_requested_nodes(&db.pool, &work, &graph, space_id, &[source_id]).await?;
    assert_eq!(
        graph
            .outgoing(owner, space_id, source_id, ListLinkReferences::default())
            .await?
            .items
            .len(),
        1
    );

    files
        .write_text(
            owner,
            space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: source_id },
                body: WriteTextBody::Encrypted(serde_json::json!({
                    "version": 1,
                    "ciphertext_b64": "opaque"
                })),
                expected_sha256: None,
            },
        )
        .await?;
    let result = project_requested_nodes(&db.pool, &work, &graph, space_id, &[source_id]).await?;
    assert_eq!(result.skipped, 1);
    assert!(
        graph
            .outgoing(owner, space_id, source_id, ListLinkReferences::default())
            .await?
            .items
            .is_empty()
    );
    assert!(
        graph
            .node_state(owner, space_id, source_id)
            .await?
            .projected_at
            .is_none()
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn manual_sync_immediately_supersedes_an_active_job() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(
        &db.pool,
        "link-stale-redispatch",
        "link-stale-redispatch@example.test",
    )
    .await?;
    let (space_id, root_id) = setup_space(
        &SpaceRepo::new(db.pool.clone()),
        owner,
        "link-stale-redispatch",
    )
    .await;
    let files_repo = FilesRepo::new(db.pool.clone());
    let files = FilesService::new(files_repo.clone());
    let work = LinkGraphWorkRepo::new(db.pool.clone());
    let graph = LinkGraphService::new(
        LinkGraphRepo::new(db.pool.clone()),
        files_repo,
        work.clone(),
    );
    let source = files
        .create_text(
            owner,
            space_id,
            CreateText {
                parent_node_id: root_id,
                name: "source.md".to_owned(),
            },
        )
        .await?;
    let source_id = source.node.node.id;
    work.request_nodes(space_id, &[source_id]).await?;
    let queue = JobQueue::new(db.pool.clone());
    let mut claimed = queue
        .claim_many(
            "link-stale-redispatch",
            &[LinkGraphProjectNodesJob::KIND.to_owned()],
            std::time::Duration::from_secs(300),
            1,
        )
        .await?;
    let first_job = claimed.pop().expect("first projection job");

    work.request_nodes(space_id, &[source_id]).await?;
    let (active_job_id, request_version, active_request_version): (Uuid, i64, i64) =
        sqlx::query_as(
            "SELECT active_job_id, request_version, active_request_version \
             FROM node_link_projection_targets \
             WHERE space_id = $1 AND node_id = $2",
        )
        .bind(space_id)
        .bind(source_id)
        .fetch_one(&db.pool)
        .await?;
    assert_ne!(active_job_id, first_job.job_id);
    assert_eq!(active_request_version, request_version);

    let result = graph
        .project_job(first_job.fence(), space_id, &[source_id])
        .await?;
    assert_eq!(result, LinkGraphProjectionBatch::default());

    let preserved_job_id: Uuid = sqlx::query_scalar(
        "SELECT active_job_id FROM node_link_projection_targets \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source_id)
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(preserved_job_id, active_job_id);

    db.cleanup().await;
    Ok(())
}
