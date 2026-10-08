//! Deletion receipts explain absence; they never replace a live presence check.
#![allow(clippy::indexing_slicing)]
mod common;

use chrono::{DateTime, Duration, Utc};
use common::{TestDb, space_with_root};
use notegate_core::{Error, security::PiiCrypto};
use notegate_db::{
    ChangeHistoryRepo, FilesRepo, PurgeRepo, SpaceRepo, SpaceUsageRepo, TextMutationKind,
    files::revisions,
};
use notegate_model::files::{StoredContent, WriteTextBody};
use serde_json::{Value, json};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn body(value: &str) -> StoredContent {
    StoredContent {
        body: WriteTextBody::Plain(value.into()),
        content_sha256: format!("{value:0<64}"),
        byte_len: value.len() as i64,
        line_count: 1,
    }
}

async fn usage(db: &TestDb, space: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT stored_bytes FROM text_revision_usage WHERE space_id = $1")
        .bind(space)
        .fetch_one(&db.pool)
        .await
}

#[tokio::test]
async fn retention_and_space_purge_serialize_without_duplicate_receipts_or_usage_release()
-> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse()?;
    let cutoff = now + Duration::days(31);
    for retention_first in [true, false] {
        let (owner, space, root) =
            space_with_root(&db.pool, &format!("revision-purge-race-{retention_first}")).await?;
        let files = FilesRepo::new(db.pool.clone()).with_revision_time(now);
        let (node, _) = files
            .insert_text(space, root, "note.md", &body("first"), owner)
            .await?;
        files
            .save_text_content(
                space,
                node.id,
                &body("second"),
                None,
                owner,
                TextMutationKind::Write,
            )
            .await?;
        let bytes = usage(&db, space).await? + body("second").byte_len;
        SpaceRepo::new(db.pool.clone())
            .delete_space(space, owner, owner)
            .await?;
        let trash = files.list_trash(owner, 100, None).await?;
        let selected = trash
            .iter()
            .find(|entry| entry.id == space)
            .ok_or("missing Space trash entry")?;
        files
            .request_trash_purge(owner, space, None, selected.into())
            .await?;

        // Hold the counter so the first real cleanup pauses inside its DELETE,
        // after acquiring the production Space gate and row lock.
        let mut blocker = db.pool.begin().await?;
        let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker)
            .await?;
        sqlx::query("SELECT space_id FROM text_revision_usage WHERE space_id = $1 FOR UPDATE")
            .bind(space)
            .fetch_one(&mut *blocker)
            .await?;
        let pool = db.pool.clone();
        let first = tokio::spawn(async move {
            if retention_first {
                revisions::cleanup_at(&pool, cutoff).await
            } else {
                PurgeRepo::new(pool)
                    .run_once()
                    .await
                    .map(|run| run.text_revisions_deleted)
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
                ).bind(blocker_pid).fetch_one(&db.pool).await?;
                if waiting { return Ok::<(), sqlx::Error>(()); }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await??;
        if retention_first {
            let blocked = PurgeRepo::new(db.pool.clone()).run_once().await?;
            assert_eq!(blocked.nodes_deleted, 0);
            assert_eq!(blocked.text_revisions_deleted, 0);
            assert!(blocked.resources_pending);
        } else {
            assert!(matches!(
                revisions::cleanup_at(&db.pool, cutoff).await,
                Err(Error::UsageRecalculationInProgress { .. })
            ));
        }
        blocker.commit().await?;
        assert_eq!(first.await??, 1);

        // Retry both paths after the winner commits. Each body releases its
        // bytes and receives a receipt exactly once, regardless of order.
        for _ in 0..2 {
            PurgeRepo::new(db.pool.clone()).run_once().await?;
            assert_eq!(revisions::cleanup_at(&db.pool, cutoff).await?, 0);
        }
        let remaining: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM text_objects WHERE space_id = $1), \
                    (SELECT count(*) FROM text_revisions WHERE space_id = $1), \
                    (SELECT COALESCE(sum(stored_bytes), 0)::bigint FROM text_revision_usage WHERE space_id = $1), \
                    (SELECT COALESCE(sum(text_bytes), 0)::bigint FROM space_storage_usage WHERE space_id = $1)",
        ).bind(space).fetch_one(&db.pool).await?;
        assert_eq!(remaining, (0, 0, 0, 0));
        let receipts: (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), count(DISTINCT resource_id), sum((metadata->>'released_bytes')::bigint)::bigint \
             FROM audit_events WHERE op_type = 'text_revision.delete' AND metadata->>'node_id' = $1",
        ).bind(node.id.to_string()).fetch_one(&db.pool).await?;
        assert_eq!(receipts, (2, 2, bytes));
        let reasons: Vec<String> = sqlx::query_scalar(
            "SELECT metadata->>'reason' FROM audit_events WHERE op_type = 'text_revision.delete' \
             AND metadata->>'node_id' = $1 ORDER BY metadata->>'reason'",
        )
        .bind(node.id.to_string())
        .fetch_all(&db.pool)
        .await?;
        assert_eq!(
            reasons,
            if retention_first {
                vec!["checkpoint_expired", "resource_purge"]
            } else {
                vec!["resource_purge", "resource_purge"]
            }
        );
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn current_body_delete_after_upgrade_is_atomic_and_records_unknown_reason() -> TestResult {
    let Some(db) = TestDb::setup_before(53).await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "current-revision-delete").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space, root, "secret.md", &body("private first"), owner)
        .await?;
    let history = ChangeHistoryRepo::new(db.pool.clone(), PiiCrypto::test());
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    let revision: Uuid = serde_json::from_value(events[0].metadata["after_revision_id"].clone())?;
    db.apply_migration(53).await?;

    sqlx::raw_sql("CREATE FUNCTION reject_current_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.op_type = 'text_revision.delete' THEN RAISE EXCEPTION 'receipt failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_current_receipt BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION reject_current_receipt();")
        .execute(&db.pool).await?;
    assert!(
        sqlx::query("DELETE FROM text_objects WHERE node_id = $1")
            .bind(node.id)
            .execute(&db.pool)
            .await
            .is_err()
    );
    assert_eq!(
        history.list_by_owner(owner, Some(space), 10, None).await?[0].metadata["after_revision_status"],
        "current"
    );
    let bytes: i64 =
        sqlx::query_scalar("SELECT text_bytes FROM space_storage_usage WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(bytes, body("private first").byte_len);
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE op_type = 'text_revision.delete'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(receipts, 0);
    sqlx::query("DROP TRIGGER reject_current_receipt ON audit_events")
        .execute(&db.pool)
        .await?;

    for expected in [1, 0] {
        assert_eq!(
            sqlx::query("DELETE FROM text_objects WHERE node_id = $1")
                .bind(node.id)
                .execute(&db.pool)
                .await?
                .rows_affected(),
            expected
        );
    }
    let receipts: Vec<(Uuid, Option<Uuid>, Value)> = sqlx::query_as(
        "SELECT owner_user_id, actor_account_id, metadata FROM audit_events \
         WHERE resource_id = $1 AND op_type = 'text_revision.delete'",
    )
    .bind(revision)
    .fetch_all(&db.pool)
    .await?;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].0, owner);
    assert_eq!(receipts[0].1, None);
    assert_eq!(receipts[0].2["reason"], "unknown");
    assert_eq!(receipts[0].2["released_bytes"], bytes);
    assert_eq!(receipts[0].2["space_id"], space.to_string());
    assert_eq!(receipts[0].2["node_id"], node.id.to_string());
    assert!(!receipts[0].2.to_string().contains("private"));
    assert!(!receipts[0].2.to_string().contains("secret"));
    let remaining: i64 =
        sqlx::query_scalar("SELECT text_bytes FROM space_storage_usage WHERE space_id = $1")
            .bind(space)
            .fetch_one(&db.pool)
            .await?;
    assert_eq!(remaining, 0);
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert_eq!(events[0].metadata["after_revision_status"], "deleted");
    assert_eq!(
        events[0].metadata["after_revision_deletion_reason"],
        "unknown"
    );
    assert!(events[0].metadata["after_revision_deleted_at"].is_string());
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn hard_purge_records_current_and_retained_revisions_only_when_deleted() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    for edited in [false, true] {
        let (owner, space, root) =
            space_with_root(&db.pool, &format!("current-purge-{edited}")).await?;
        let files = FilesRepo::new(db.pool.clone());
        let (node, _) = files
            .insert_text(space, root, "note.md", &body("first"), owner)
            .await?;
        if edited {
            files
                .save_text_content(
                    space,
                    node.id,
                    &body("second"),
                    None,
                    owner,
                    TextMutationKind::Write,
                )
                .await?;
        }
        let history = ChangeHistoryRepo::new(db.pool.clone(), PiiCrypto::test());
        let events = history.list_by_owner(owner, Some(space), 10, None).await?;
        let revisions: Vec<Uuid> = events
            .iter()
            .map(|event| serde_json::from_value(event.metadata["after_revision_id"].clone()))
            .collect::<Result<_, _>>()?;
        assert_eq!(revisions.len(), if edited { 2 } else { 1 });
        assert_eq!(events[0].metadata["after_revision_status"], "current");
        if edited {
            assert_eq!(events[0].metadata["before_revision_status"], "retained");
        }
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE resource_id = ANY($1) AND op_type = 'text_revision.delete'")
            .bind(&revisions).fetch_one(&db.pool).await?;
        assert_eq!(receipts, 0, "creation and ordinary saves are not deletions");

        files.soft_delete_node(space, node.id, owner, false).await?;
        let trash = files.list_trash(owner, 100, None).await?;
        let entry = trash
            .iter()
            .find(|entry| entry.id == node.id)
            .ok_or("missing trash entry")?;
        files
            .request_trash_purge(owner, space, Some(node.id), entry.into())
            .await?;
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE resource_id = ANY($1) AND op_type = 'text_revision.delete'")
            .bind(&revisions).fetch_one(&db.pool).await?;
        assert_eq!(
            receipts, 0,
            "trash and purge requests are not completed deletions"
        );

        for _ in 0..2 {
            PurgeRepo::new(db.pool.clone()).run_once().await?;
            let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE resource_id = ANY($1) AND op_type = 'text_revision.delete'")
                .bind(&revisions).fetch_one(&db.pool).await?;
            assert_eq!(
                receipts as usize,
                revisions.len(),
                "retries do not duplicate receipts"
            );
        }
        let events = history.list_by_owner(owner, Some(space), 10, None).await?;
        for metadata in events.iter().map(|event| &event.metadata) {
            for prefix in ["before", "after"] {
                if metadata.get(format!("{prefix}_revision_id")).is_some() {
                    assert_eq!(metadata[format!("{prefix}_revision_status")], "deleted");
                    assert_eq!(
                        metadata[format!("{prefix}_revision_deletion_reason")],
                        "resource_purge"
                    );
                    assert!(metadata[format!("{prefix}_revision_deleted_at")].is_string());
                }
            }
        }
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn direct_delete_is_atomic_and_missing_receipts_do_not_imply_retention() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "direct-revision-delete").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space, root, "secret.md", &body("private first"), owner)
        .await?;
    files
        .save_text_content(
            space,
            node.id,
            &body("private second"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    let history = ChangeHistoryRepo::new(db.pool.clone(), PiiCrypto::test());
    let before = history.list_by_owner(owner, Some(space), 10, None).await?;
    let revision: Uuid = serde_json::from_value(before[0].metadata["before_revision_id"].clone())?;
    let retained_bytes = usage(&db, space).await?;
    assert!(retained_bytes > 0);

    // A failed receipt must roll back the DELETE and its accounting trigger.
    sqlx::raw_sql("CREATE FUNCTION reject_revision_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.op_type = 'text_revision.delete' THEN RAISE EXCEPTION 'receipt failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_revision_receipt BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION reject_revision_receipt();")
        .execute(&db.pool).await?;
    assert!(
        sqlx::query("DELETE FROM text_revisions WHERE id = $1")
            .bind(revision)
            .execute(&db.pool)
            .await
            .is_err()
    );
    assert_eq!(usage(&db, space).await?, retained_bytes);
    assert_eq!(
        history.list_by_owner(owner, Some(space), 10, None).await?[0].metadata["before_revision_status"],
        "retained"
    );
    sqlx::query("DROP TRIGGER reject_revision_receipt ON audit_events")
        .execute(&db.pool)
        .await?;

    assert_eq!(
        sqlx::query("DELETE FROM text_revisions WHERE id = $1")
            .bind(revision)
            .execute(&db.pool)
            .await?
            .rows_affected(),
        1
    );
    assert_eq!(usage(&db, space).await?, 0);
    assert!(matches!(
        files.read_text_revision(space, node.id, revision).await,
        Err(notegate_core::Error::NotFound(_))
    ));
    let (receipt_owner, actor, metadata): (Uuid, Option<Uuid>, Value) =
        sqlx::query_as("SELECT owner_user_id, actor_account_id, metadata FROM audit_events WHERE resource_id = $1 AND op_type = 'text_revision.delete'")
            .bind(revision).fetch_one(&db.pool).await?;
    assert_eq!(receipt_owner, owner);
    assert_eq!(actor, None);
    assert_eq!(metadata["reason"], "unknown");
    assert_eq!(metadata["released_bytes"], retained_bytes);
    assert!(!metadata.to_string().contains("private"));
    assert!(!metadata.to_string().contains("secret"));
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert_eq!(events[0].metadata["before_revision_status"], "deleted");
    assert_eq!(
        events[0].metadata["before_revision_deletion_reason"],
        "unknown"
    );
    assert!(events[0].metadata["before_revision_deleted_at"].is_string());
    assert_eq!(events[0].metadata["after_revision_status"], "current");
    assert!(
        history
            .list_by_owner(Uuid::new_v4(), None, 10, None)
            .await?
            .is_empty()
    );

    // Retried deletion cannot manufacture a second receipt.
    sqlx::query("DELETE FROM text_revisions WHERE id = $1")
        .bind(revision)
        .execute(&db.pool)
        .await?;
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE resource_id = $1 AND op_type = 'text_revision.delete'")
        .bind(revision).fetch_one(&db.pool).await?;
    assert_eq!(receipts, 1);

    // A missing/expired receipt must not fabricate a policy reason or timestamp.
    sqlx::query("DELETE FROM audit_events WHERE resource_id = $1")
        .bind(revision)
        .execute(&db.pool)
        .await?;
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert_eq!(events[0].metadata["before_revision_status"], "unavailable");
    assert!(events[0].metadata["before_revision_deleted_at"].is_null());
    assert!(events[0].metadata["before_revision_deletion_reason"].is_null());

    // A receipt for the same ID in a different scope cannot explain this absence.
    sqlx::query("INSERT INTO audit_events(source, op_type, resource_type, resource_id, metadata) VALUES ('system', 'text_revision.delete', 'text_revision', $1, $2)")
        .bind(revision).bind(json!({"space_id": Uuid::new_v4(), "node_id": node.id, "reason": "resource_purge"}))
        .execute(&db.pool).await?;
    assert_eq!(
        history.list_by_owner(owner, Some(space), 10, None).await?[0].metadata["before_revision_status"],
        "unavailable"
    );

    // Simulate restored data alongside an old receipt: the current body wins.
    let current: Uuid = serde_json::from_value(events[0].metadata["after_revision_id"].clone())?;
    sqlx::query("INSERT INTO audit_events(source, op_type, resource_type, resource_id, metadata) VALUES ('system', 'text_revision.delete', 'text_revision', $1, $2)")
        .bind(current).bind(json!({"space_id": space, "node_id": node.id, "reason": "unknown"}))
        .execute(&db.pool).await?;
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert_eq!(events[0].metadata["after_revision_status"], "current");
    assert_eq!(events[0].metadata["after_revision_deletion_conflict"], true);

    // A DB failure is an error, never a successful "unavailable" response.
    sqlx::query("ALTER TABLE audit_events RENAME TO temporarily_unavailable_audit")
        .execute(&db.pool)
        .await?;
    assert!(
        history
            .list_by_owner(owner, Some(space), 10, None)
            .await
            .is_err()
    );
    sqlx::query("ALTER TABLE temporarily_unavailable_audit RENAME TO audit_events")
        .execute(&db.pool)
        .await?;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn retention_records_the_executed_policy_and_respects_persisted_deadlines() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "revision-policy-receipts").await?;
    let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse()?;
    let files = FilesRepo::new(db.pool.clone()).with_revision_time(now);
    let (node, _) = files
        .insert_text(space, root, "note.md", &body("a"), owner)
        .await?;
    let editing = files.with_revision_context("browser", Some(Uuid::new_v4()));
    for (seconds, text) in [(60, "b"), (120, "c")] {
        editing
            .clone()
            .with_revision_time(now + Duration::seconds(seconds))
            .save_text_content(
                space,
                node.id,
                &body(text),
                None,
                owner,
                TextMutationKind::Write,
            )
            .await?;
    }
    // Policy migration/maintenance can explicitly extend an existing deadline.
    sqlx::query("UPDATE text_revisions SET cleanup_at = $2 WHERE node_id = $1 AND NOT checkpoint")
        .bind(node.id)
        .bind(now + Duration::days(2))
        .execute(&db.pool)
        .await?;
    assert_eq!(
        revisions::cleanup_at(&db.pool, now + Duration::days(1) + Duration::minutes(3)).await?,
        0
    );
    assert_eq!(
        revisions::cleanup_at(&db.pool, now + Duration::days(2)).await?,
        1
    );
    assert_eq!(
        revisions::cleanup_at(&db.pool, now + Duration::days(31)).await?,
        1
    );
    let reasons: Vec<String> = sqlx::query_scalar("SELECT metadata->>'reason' FROM audit_events WHERE op_type = 'text_revision.delete' ORDER BY id")
        .fetch_all(&db.pool).await?;
    assert_eq!(reasons, ["intermediate_expired", "checkpoint_expired"]);
    assert_eq!(usage(&db, space).await?, 0);

    // Transaction-local attribution must not leak into a later ordinary DELETE.
    editing
        .with_revision_time(now + Duration::days(32))
        .save_text_content(
            space,
            node.id,
            &body("d"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    sqlx::query("DELETE FROM text_revisions WHERE node_id = $1")
        .bind(node.id)
        .execute(&db.pool)
        .await?;
    let reason: String = sqlx::query_scalar("SELECT metadata->>'reason' FROM audit_events WHERE op_type = 'text_revision.delete' ORDER BY id DESC LIMIT 1")
        .fetch_one(&db.pool).await?;
    assert_eq!(reason, "unknown");
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn usage_reconciliation_repairs_revision_drift_without_recreating_bodies() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "revision-usage-repair").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space, root, "note.md", &body("first"), owner)
        .await?;
    files
        .save_text_content(
            space,
            node.id,
            &body("second"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    files.soft_delete_node(space, node.id, owner, false).await?;
    let bytes = usage(&db, space).await?;
    sqlx::query("DELETE FROM text_revision_usage WHERE space_id = $1")
        .bind(space)
        .execute(&db.pool)
        .await?;
    let reconcile = SpaceUsageRepo::new(db.pool.clone());
    reconcile.reconcile_space(space).await?;
    assert_eq!(usage(&db, space).await?, bytes);

    // TRUNCATE bypasses row DELETE triggers. Repair capacity, not history.
    sqlx::query("TRUNCATE text_revisions")
        .execute(&db.pool)
        .await?;
    assert_eq!(usage(&db, space).await?, bytes);
    reconcile.reconcile_space(space).await?;
    assert_eq!(usage(&db, space).await?, 0);
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE op_type = 'text_revision.delete'",
    )
    .fetch_one(&db.pool)
    .await?;
    assert_eq!(receipts, 0);
    let history = ChangeHistoryRepo::new(db.pool.clone(), PiiCrypto::test());
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert!(
        events
            .iter()
            .any(|e| e.metadata["before_revision_status"] == "unavailable")
    );
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn cascade_receipts_keep_the_owner_and_expire_at_the_audit_boundary() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "revision-cascade-receipts").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space, root, "note.md", &body("first"), owner)
        .await?;
    files
        .save_text_content(
            space,
            node.id,
            &body("second"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    sqlx::query("DELETE FROM spaces WHERE id = $1")
        .bind(space)
        .execute(&db.pool)
        .await?;
    let receipts: Vec<(i64, Uuid, DateTime<Utc>, String)> =
        sqlx::query_as("SELECT id, owner_user_id, created_at, metadata->>'reason' FROM audit_events WHERE op_type = 'text_revision.delete'")
            .fetch_all(&db.pool).await?;
    assert_eq!(
        receipts.len(),
        2,
        "current and historical bodies were deleted"
    );
    for (_, receipt_owner, _, reason) in &receipts {
        assert_eq!(*receipt_owner, owner);
        assert_eq!(
            reason, "unknown",
            "a direct cascade is not proof of policy-driven purge"
        );
    }
    let history = ChangeHistoryRepo::new(db.pool.clone(), PiiCrypto::test());
    let events = history.list_by_owner(owner, Some(space), 10, None).await?;
    assert_eq!(events[0].metadata["before_revision_status"], "deleted");
    assert_eq!(events[0].metadata["after_revision_status"], "deleted");
    let (id, _, created_at, _) = &receipts[0];
    let cutoff = *created_at + Duration::days(180);
    for (time, expected) in [(cutoff - Duration::microseconds(1), true), (cutoff, false)] {
        PurgeRepo::new(db.pool.clone())
            .with_history_time(time)
            .run_once()
            .await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE id = $1)")
                .bind(id)
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(exists, expected);
    }
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn usage_repair_waits_for_a_direct_delete_before_taking_its_snapshot() -> TestResult {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let (owner, space, root) = space_with_root(&db.pool, "revision-concurrent-repair").await?;
    let files = FilesRepo::new(db.pool.clone());
    let (node, _) = files
        .insert_text(space, root, "note.md", &body("first"), owner)
        .await?;
    files
        .save_text_content(
            space,
            node.id,
            &body("second"),
            None,
            owner,
            TextMutationKind::Write,
        )
        .await?;
    {
        let mut blocker = db.pool.begin().await?;
        sqlx::query("SELECT space_id FROM text_revision_usage WHERE space_id = $1 FOR UPDATE")
            .bind(space)
            .fetch_one(&mut *blocker)
            .await?;
        let mut deletion_connection = db.pool.acquire().await?;
        let deletion_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *deletion_connection)
            .await?;
        let deletion = sqlx::query("DELETE FROM text_revisions WHERE space_id = $1")
            .bind(space)
            .execute(&mut *deletion_connection);
        tokio::pin!(deletion);
        // Drive DELETE until its row trigger is blocked on the counter held above.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut deletion)
                .await
                .is_err()
        );
        let waiting: bool = sqlx::query_scalar(
            "SELECT wait_event_type = 'Lock' FROM pg_stat_activity WHERE pid = $1",
        )
        .bind(deletion_pid)
        .fetch_one(&db.pool)
        .await?;
        assert!(waiting);

        let reconcile = SpaceUsageRepo::new(db.pool.clone());
        let repair = reconcile.reconcile_space(space);
        tokio::pin!(repair);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut repair)
                .await
                .is_err()
        );
        blocker.commit().await?;
        let (deleted, repaired) = tokio::join!(deletion, repair);
        assert_eq!(deleted?.rows_affected(), 1);
        repaired?;
    }
    assert_eq!(usage(&db, space).await?, 0);
    db.cleanup().await;
    Ok(())
}
