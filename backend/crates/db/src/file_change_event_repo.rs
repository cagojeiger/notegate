//! File-change event persistence: insert plus space/node-scoped listing for
//! event history.

use crate::event_history_query::{EventCursorPosition, UuidFilter, list_event_rows};
use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::FileChangeEventCursor;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Write-side row for capture; the read shape is `notegate_model::FileChangeEvent`.
#[derive(Debug)]
pub(crate) struct NewFileChangeEvent {
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub op_type: &'static str,
    pub metadata: Value,
}

/// Insert one file-change event row.
pub(crate) async fn insert_file_change_event(
    tx: &mut sqlx::PgConnection,
    event: NewFileChangeEvent,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO file_change_events \
         (space_id, node_id, actor_account_id, op_type, metadata) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event.space_id)
    .bind(event.node_id)
    .bind(event.actor_account_id)
    .bind(event.op_type)
    .bind(event.metadata)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}

/// List file-change events for `space_id` (optionally scoped to `node_id`),
/// newest first. Matches the `file_change_events_space_time_idx` /
/// `file_change_events_node_time_idx` order.
pub(crate) async fn list_file_change_events(
    pool: &PgPool,
    space_id: Uuid,
    node_id: Option<Uuid>,
    limit: i64,
    cursor: Option<&FileChangeEventCursor>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    let rows = list_event_rows::<FileChangeEventRow>(
        pool,
        "file_change_events",
        FILE_CHANGE_EVENT_COLUMNS,
        UuidFilter::new("space_id", space_id),
        node_id.map(|node_id| UuidFilter::new("node_id", node_id)),
        limit,
        cursor.map(|cursor| EventCursorPosition {
            created_at: cursor.created_at,
            id: cursor.id,
        }),
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(notegate_model::FileChangeEvent::from)
        .collect())
}

#[derive(Debug)]
pub struct FileChangeSyncRows {
    pub events: Vec<notegate_model::FileChangeEvent>,
    pub latest_id: i64,
    pub token_valid: bool,
}

/// Read file-change events after a space-scoped sync token, oldest first.
///
/// `id` is globally increasing, but the token is accepted only when it belongs
/// to this Space. A missing token indicates that retained history can no longer
/// prove a lossless continuation. File-tree commands hold the Space mutation
/// lock through event insert and commit, so `id` order is commit-stable within
/// one Space.
pub(crate) async fn sync_file_change_events(
    pool: &PgPool,
    space_id: Uuid,
    after_id: Option<i64>,
    limit: i64,
) -> Result<FileChangeSyncRows> {
    let (latest_id, token_valid) = sqlx::query_as::<_, (i64, bool)>(
        "SELECT \
            COALESCE(( \
                SELECT id FROM file_change_events \
                WHERE space_id = $1 ORDER BY id DESC LIMIT 1 \
            ), 0), \
            ($2::bigint IS NULL OR $2 = 0 OR EXISTS( \
                SELECT 1 FROM file_change_events WHERE space_id = $1 AND id = $2 \
            ))",
    )
    .bind(space_id)
    .bind(after_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    let Some(after_id) = after_id else {
        return Ok(FileChangeSyncRows {
            events: Vec::new(),
            latest_id,
            token_valid: true,
        });
    };

    if !token_valid {
        return Ok(FileChangeSyncRows {
            events: Vec::new(),
            latest_id,
            token_valid: false,
        });
    }

    if after_id == latest_id {
        return Ok(FileChangeSyncRows {
            events: Vec::new(),
            latest_id,
            token_valid: true,
        });
    }

    let rows = sqlx::query_as::<_, FileChangeEventRow>(&format!(
        "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
         WHERE space_id = $1 AND id > $2 \
         ORDER BY id ASC LIMIT $3"
    ))
    .bind(space_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(FileChangeSyncRows {
        events: rows
            .into_iter()
            .map(notegate_model::FileChangeEvent::from)
            .collect(),
        latest_id,
        token_valid: true,
    })
}

#[derive(Debug, FromRow)]
struct FileChangeEventRow {
    id: i64,
    created_at: DateTime<Utc>,
    space_id: Uuid,
    node_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    op_type: String,
    metadata: Value,
}

impl From<FileChangeEventRow> for notegate_model::FileChangeEvent {
    fn from(row: FileChangeEventRow) -> Self {
        Self {
            id: row.id,
            created_at: row.created_at,
            space_id: row.space_id,
            node_id: row.node_id,
            actor_account_id: row.actor_account_id,
            op_type: row.op_type,
            metadata: row.metadata,
        }
    }
}

const FILE_CHANGE_EVENT_COLUMNS: &str =
    "id, created_at, space_id, node_id, actor_account_id, op_type, metadata";
