//! Integration tests for the self-review `AuditEventRepo` read API.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]
mod common;

use chrono::{DateTime, Duration, Utc};
use common::{TestDb, insert_user_account};
use notegate_db::AuditEventRepo;
use notegate_model::AuditEventCursor;
use uuid::Uuid;

/// Insert one audit event row directly, bypassing the (crate-private) capture
/// path so the test can control `occurred_at` ordering deterministically.
async fn insert_event(
    pool: &sqlx::PgPool,
    owner_user_id: Uuid,
    op_type: &str,
    occurred_at: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO audit_events \
         (owner_user_id, actor_account_id, source, op_type, resource_type, resource_id, occurred_at, metadata) \
         VALUES ($1, $1, 'rest', $2, 'space', gen_random_uuid(), $3, '{}'::jsonb) \
         RETURNING id",
    )
    .bind(owner_user_id)
    .bind(op_type)
    .bind(occurred_at)
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn list_by_owner_returns_newest_first() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "order-user", "order-user@example.test").await?;
    let repo = AuditEventRepo::new(db.pool.clone());

    let base = Utc::now();
    insert_event(&db.pool, owner, "space.create", base).await?;
    insert_event(&db.pool, owner, "space.update", base + Duration::seconds(1)).await?;
    insert_event(&db.pool, owner, "space.delete", base + Duration::seconds(2)).await?;

    let page = repo.list_by_owner(owner, 10, None).await?;
    let op_types = page.iter().map(|e| e.op_type.as_str()).collect::<Vec<_>>();
    assert_eq!(
        op_types,
        vec!["space.delete", "space.update", "space.create"],
        "newest occurred_at first"
    );

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_owner_pages_by_cursor_without_gaps_or_duplicates()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner = insert_user_account(&db.pool, "page-user", "page-user@example.test").await?;
    let repo = AuditEventRepo::new(db.pool.clone());

    let base = Utc::now();
    let mut inserted_ids = Vec::new();
    for index in 0..5 {
        let id = insert_event(
            &db.pool,
            owner,
            "space.update",
            base + Duration::seconds(index),
        )
        .await?;
        inserted_ids.push(id);
    }
    inserted_ids.reverse(); // expected newest-first order

    let first_page = repo.list_by_owner(owner, 2, None).await?;
    assert_eq!(first_page.len(), 2);
    assert_eq!(
        first_page.iter().map(|e| e.id).collect::<Vec<_>>(),
        inserted_ids[0..2]
    );

    let cursor = AuditEventCursor {
        occurred_at: first_page.last().expect("last item").occurred_at,
        id: first_page.last().expect("last item").id,
    };
    let second_page = repo.list_by_owner(owner, 2, Some(&cursor)).await?;
    assert_eq!(second_page.len(), 2);
    assert_eq!(
        second_page.iter().map(|e| e.id).collect::<Vec<_>>(),
        inserted_ids[2..4]
    );

    let cursor2 = AuditEventCursor {
        occurred_at: second_page.last().expect("last item").occurred_at,
        id: second_page.last().expect("last item").id,
    };
    let third_page = repo.list_by_owner(owner, 2, Some(&cursor2)).await?;
    assert_eq!(third_page.len(), 1);
    assert_eq!(third_page[0].id, inserted_ids[4]);

    let mut all_ids = first_page
        .iter()
        .chain(second_page.iter())
        .chain(third_page.iter())
        .map(|e| e.id)
        .collect::<Vec<_>>();
    let mut expected_ids = inserted_ids.clone();
    all_ids.sort_unstable();
    expected_ids.sort_unstable();
    assert_eq!(all_ids, expected_ids, "pages cover every row exactly once");

    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn list_by_owner_excludes_other_owners_events() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let owner_a = insert_user_account(&db.pool, "scope-a", "scope-a@example.test").await?;
    let owner_b = insert_user_account(&db.pool, "scope-b", "scope-b@example.test").await?;
    let repo = AuditEventRepo::new(db.pool.clone());

    let now = Utc::now();
    insert_event(&db.pool, owner_a, "space.create", now).await?;
    insert_event(&db.pool, owner_b, "space.create", now).await?;
    insert_event(
        &db.pool,
        owner_b,
        "space.delete",
        now + Duration::seconds(1),
    )
    .await?;

    let events_a = repo.list_by_owner(owner_a, 10, None).await?;
    assert_eq!(events_a.len(), 1);
    assert_eq!(events_a[0].op_type, "space.create");

    let events_b = repo.list_by_owner(owner_b, 10, None).await?;
    assert_eq!(events_b.len(), 2);
    assert!(
        events_b
            .iter()
            .all(|event| event.actor_account_id == Some(owner_b)),
        "owner_b's page must not leak owner_a's events"
    );

    db.cleanup().await;
    Ok(())
}
