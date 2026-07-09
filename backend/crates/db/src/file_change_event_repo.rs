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
