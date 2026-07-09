use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_model::FileChangeEventCursor;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct NewFileChangeEvent {
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub op_type: &'static str,
    pub metadata: Value,
}

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

pub(crate) async fn list_file_change_events(
    pool: &PgPool,
    space_id: Uuid,
    node_id: Option<Uuid>,
    limit: i64,
    cursor: Option<&FileChangeEventCursor>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    let rows = match (node_id, cursor) {
        (None, None) => {
            sqlx::query_as::<_, FileChangeEventRow>(&format!(
                "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
                 WHERE space_id = $1 \
                 ORDER BY created_at DESC, id DESC LIMIT $2"
            ))
            .bind(space_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        (None, Some(cursor)) => {
            sqlx::query_as::<_, FileChangeEventRow>(&format!(
                "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
                 WHERE space_id = $1 \
                   AND (created_at, id) < ($2, $3) \
                 ORDER BY created_at DESC, id DESC LIMIT $4"
            ))
            .bind(space_id)
            .bind(cursor.created_at)
            .bind(cursor.id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        (Some(node_id), None) => {
            sqlx::query_as::<_, FileChangeEventRow>(&format!(
                "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
                 WHERE space_id = $1 AND node_id = $2 \
                 ORDER BY created_at DESC, id DESC LIMIT $3"
            ))
            .bind(space_id)
            .bind(node_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        (Some(node_id), Some(cursor)) => {
            sqlx::query_as::<_, FileChangeEventRow>(&format!(
                "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
                 WHERE space_id = $1 AND node_id = $2 \
                   AND (created_at, id) < ($3, $4) \
                 ORDER BY created_at DESC, id DESC LIMIT $5"
            ))
            .bind(space_id)
            .bind(node_id)
            .bind(cursor.created_at)
            .bind(cursor.id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
    .map_err(map_sqlx_error)?;

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
