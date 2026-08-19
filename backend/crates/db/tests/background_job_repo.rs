//! Integration coverage for account-scoped background job history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::unwrap_in_result)]
mod common;

use chrono::Utc;
use common::{TestDb, space_with_root};
use notegate_db::BackgroundJobRepo;
use notegate_model::BackgroundJobCursor;
use uuid::Uuid;

#[tokio::test]
async fn lists_owned_visible_jobs_and_loads_attempts() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner_id, space_id, _) = space_with_root(&db.pool, "jobs-owned").await?;
    let (_, other_space_id, _) = space_with_root(&db.pool, "jobs-other").await?;
    let owned_job = insert_succeeded_job(&db.pool, "space_usage_reconcile", space_id, true).await?;
    insert_succeeded_attempt(&db.pool, owned_job).await?;
    let generic_job = insert_succeeded_job(&db.pool, "document_export", space_id, true).await?;
    let link_job =
        insert_succeeded_job(&db.pool, "link_graph_project_nodes", space_id, true).await?;
    insert_succeeded_attempt(&db.pool, link_job).await?;
    insert_succeeded_job(&db.pool, "space_usage_reconcile", other_space_id, true).await?;
    insert_succeeded_job(&db.pool, "link_graph_project_nodes", other_space_id, true).await?;
    insert_succeeded_job(&db.pool, "internal_maintenance", space_id, false).await?;

    let repo = BackgroundJobRepo::new(db.pool.clone());
    let jobs = repo.list_by_owner(owner_id, 10, None).await?;
    assert_eq!(jobs.len(), 3);
    assert!(jobs.iter().any(|job| job.id == owned_job));
    assert!(jobs.iter().any(|job| job.id == generic_job));
    assert!(jobs.iter().any(|job| job.id == link_job));
    assert!(
        jobs.iter()
            .all(|job| job.context_kind.as_deref() == Some("space"))
    );
    assert!(
        jobs.iter()
            .all(|job| job.context_label.as_deref() == Some("ws-jobs-owned"))
    );

    let detail = repo
        .get_by_owner(owner_id, owned_job)
        .await?
        .expect("owned job detail");
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(
        detail
            .attempts
            .first()
            .and_then(|attempt| attempt.outcome.as_deref()),
        Some("succeeded")
    );
    let link_detail = repo
        .get_by_owner(owner_id, link_job)
        .await?
        .expect("owned link job detail");
    assert_eq!(link_detail.job.context_kind.as_deref(), Some("space"));
    assert_eq!(link_detail.job.context_id, Some(space_id));
    assert_eq!(
        link_detail.job.context_label.as_deref(),
        Some("ws-jobs-owned")
    );
    assert_eq!(link_detail.attempts.len(), 1);

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn pages_jobs_by_created_at_and_id() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner_id, space_id, _) = space_with_root(&db.pool, "jobs-page").await?;
    let older = insert_succeeded_job(&db.pool, "space_usage_reconcile", space_id, true).await?;
    sqlx::query(
        "UPDATE background_jobs SET created_at = now() - interval '1 minute' WHERE job_id = $1",
    )
    .bind(older)
    .execute(&db.pool)
    .await?;
    let newer = insert_succeeded_job(&db.pool, "space_usage_reconcile", space_id, true).await?;

    let repo = BackgroundJobRepo::new(db.pool.clone());
    let first = repo.list_by_owner(owner_id, 1, None).await?;
    let first_job = first.first().expect("first job page");
    assert_eq!(first_job.id, newer);
    let cursor = BackgroundJobCursor {
        created_at: first_job.created_at,
        id: first_job.id,
    };
    let second = repo.list_by_owner(owner_id, 1, Some(&cursor)).await?;
    assert_eq!(second.first().expect("second job page").id, older);

    db.cleanup().await;
    Ok(())
}

async fn insert_succeeded_job(
    pool: &sqlx::PgPool,
    kind: &str,
    space_id: Uuid,
    visible_in_history: bool,
) -> Result<Uuid, sqlx::Error> {
    let (owner_account_id, context_label): (Uuid, String) =
        sqlx::query_as("SELECT owner_user_id, name FROM spaces WHERE id = $1")
            .bind(space_id)
            .fetch_one(pool)
            .await?;
    sqlx::query_scalar(
        "INSERT INTO background_jobs \
         (job_kind, payload, status, attempt_count, max_attempts, completed_at, \
          history_visibility, history_owner_account_id, context_kind, context_id, context_label) \
         VALUES ($1, jsonb_build_object('space_id', $2), 'succeeded', 1, 8, now(), \
                 $3, $4, $5, $2, $6) \
         RETURNING job_id",
    )
    .bind(kind)
    .bind(space_id)
    .bind(if visible_in_history {
        "visible"
    } else {
        "hidden"
    })
    .bind(visible_in_history.then_some(owner_account_id))
    .bind(visible_in_history.then_some("space"))
    .bind(visible_in_history.then_some(context_label))
    .fetch_one(pool)
    .await
}

async fn insert_succeeded_attempt(pool: &sqlx::PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO background_job_attempts \
         (job_id, attempt_number, claim_token, worker_id, started_at, finished_at, outcome) \
         VALUES ($1, 1, $2, 'test-worker', $3, $3, 'succeeded')",
    )
    .bind(job_id)
    .bind(Uuid::new_v4())
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}
