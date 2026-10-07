//! Reclaim terminal object records and events after their retention windows.

use chrono::{DateTime, Utc};

use crate::map_sqlx_error;
use notegate_core::{Result, limits};
use sqlx::{PgPool, Row as _};

const OBJECT_STORAGE_HISTORY_PURGE_BATCH: i64 = 1_000;
const AUDIT_EVENT_PURGE_BATCH: i64 = 1_000;
const FILE_CHANGE_EVENT_PURGE_BATCH: i64 = 1_000;
const COMMAND_INVOCATION_PURGE_BATCH: i64 = 1_000;

pub(super) struct PurgedHistory {
    pub(super) object_storage_history_deleted: i64,
    pub(super) audit_events_deleted: i64,
    pub(super) file_change_events_deleted: i64,
    pub(super) command_invocations_deleted: i64,
}

pub(super) async fn purge(pool: &PgPool, now: Option<DateTime<Utc>>) -> Result<PurgedHistory> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let object_storage_history_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM object_storage_objects \
             WHERE state IN ('expired','deleted') \
               AND COALESCE(deleted_at, last_activity_at) \
                   <= COALESCE($3::timestamptz, now()) - make_interval(days => $1::int) \
             ORDER BY COALESCE(deleted_at, last_activity_at), id \
             LIMIT $2 \
             FOR UPDATE SKIP LOCKED \
         ), deleted AS ( \
             DELETE FROM object_storage_objects o USING due \
             WHERE o.id = due.id \
             RETURNING o.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(i32::try_from(limits::OBJECT_STORAGE_HISTORY_RETENTION_DAYS).unwrap_or(i32::MAX))
    .bind(OBJECT_STORAGE_HISTORY_PURGE_BATCH)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    let audit_events_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM audit_events \
             WHERE created_at <= COALESCE($3::timestamptz, now()) - make_interval(days => $1::int) \
             ORDER BY created_at, id \
             LIMIT $2 \
         ), deleted AS ( \
             DELETE FROM audit_events e USING due \
             WHERE e.id = due.id \
             RETURNING e.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(i32::try_from(limits::AUDIT_EVENT_RETENTION_DAYS).unwrap_or(i32::MAX))
    .bind(AUDIT_EVENT_PURGE_BATCH)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    let file_change_events_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM file_change_events \
             WHERE created_at <= COALESCE($3::timestamptz, now()) - make_interval(days => $1::int) \
             ORDER BY created_at, id \
             LIMIT $2 \
         ), deleted AS ( \
             DELETE FROM file_change_events e USING due \
             WHERE e.id = due.id \
             RETURNING e.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(i32::try_from(limits::FILE_CHANGE_EVENT_RETENTION_DAYS).unwrap_or(i32::MAX))
    .bind(FILE_CHANGE_EVENT_PURGE_BATCH)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    let command_invocations_deleted: i64 = sqlx::query(
        "WITH due AS ( \
             SELECT id FROM command_invocations \
             WHERE created_at <= COALESCE($3::timestamptz, now()) - make_interval(days => $1::int) \
             ORDER BY created_at, id \
             LIMIT $2 \
         ), deleted AS ( \
             DELETE FROM command_invocations i USING due \
             WHERE i.id = due.id \
             RETURNING i.id \
         ) \
         SELECT count(*) AS deleted_count FROM deleted",
    )
    .bind(i32::try_from(limits::COMMAND_INVOCATION_RETENTION_DAYS).unwrap_or(i32::MAX))
    .bind(COMMAND_INVOCATION_PURGE_BATCH)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .get("deleted_count");

    // Accounting scopes outlive deleted Spaces while any object still needs
    // cleanup. Empty orphan counters carry no retention obligation themselves.
    let storage_scopes_deleted = sqlx::query(
        "WITH due AS (SELECT u.space_id FROM space_storage_usage u \
             WHERE u.text_bytes = 0 AND u.file_bytes = 0 \
               AND NOT EXISTS (SELECT 1 FROM spaces s WHERE s.id = u.space_id) \
               AND NOT EXISTS (SELECT 1 FROM object_storage_objects o WHERE o.usage_space_id = u.space_id \
                   AND o.state NOT IN ('expired', 'deleted')) \
             ORDER BY u.space_id LIMIT 1000 FOR UPDATE OF u SKIP LOCKED) \
         DELETE FROM space_storage_usage u USING due WHERE u.space_id = due.space_id",
    ).execute(&mut *tx).await.map_err(map_sqlx_error)?.rows_affected();
    tx.commit().await.map_err(map_sqlx_error)?;
    tracing::info!(
        event = "purge.group_completed",
        group = "history",
        object_storage_history_deleted,
        audit_events_deleted,
        file_change_events_deleted,
        command_invocations_deleted,
        storage_scopes_deleted,
    );

    Ok(PurgedHistory {
        object_storage_history_deleted,
        audit_events_deleted,
        file_change_events_deleted,
        command_invocations_deleted,
    })
}
