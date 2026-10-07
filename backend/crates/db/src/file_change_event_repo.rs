//! File-change event persistence: insert plus space/node-scoped listing for
//! event history.

use crate::event_history_query::{
    EventCursorPosition, UuidFilter, list_event_rows, list_event_rows_by_id,
};
use crate::map_sqlx_error;
use chrono::{DateTime, Utc};
use notegate_core::{
    Error, Result,
    security::{EncryptedHistoryValue, PiiCrypto},
};
use notegate_model::FileChangeEventCursor;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ChangeHistoryRepo {
    pool: PgPool,
    crypto: PiiCrypto,
}
impl ChangeHistoryRepo {
    pub fn new(pool: PgPool, crypto: PiiCrypto) -> Self {
        Self { pool, crypto }
    }
    pub async fn encrypt_legacy_metadata(&self) -> Result<u64> {
        encrypt_legacy(&self.pool, &self.crypto).await
    }
    pub async fn list_by_owner(
        &self,
        owner: Uuid,
        space: Option<Uuid>,
        limit: i64,
        cursor: Option<&FileChangeEventCursor>,
    ) -> Result<Vec<notegate_model::FileChangeEvent>> {
        list_by_owner(&self.pool, &self.crypto, owner, space, limit, cursor).await
    }
}

/// Write-side row for capture; the read shape is `notegate_model::FileChangeEvent`.
#[derive(Debug)]
pub(crate) struct NewFileChangeEvent {
    pub operation_id: Option<Uuid>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub op_type: &'static str,
    pub metadata: Value,
}

/// Insert one file-change event row.
pub(crate) async fn insert_file_change_event(
    tx: &mut sqlx::PgConnection,
    crypto: &PiiCrypto,
    event: NewFileChangeEvent,
) -> Result<()> {
    // A dedicated snapshot UUID binds AEAD before INSERT, without another DB
    // round trip. The sequence ID still defines the Space's mutation order.
    let snapshot_id = Uuid::new_v4();
    let (metadata, private) =
        protect_metadata(crypto, event.space_id, snapshot_id, event.metadata)?;
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO file_change_events \
         (space_id, node_id, actor_account_id, op_type, metadata, operation_id, owner_user_id, private_metadata, snapshot_id) \
         SELECT s.id, $2, $3, $4, \
           $5::jsonb || jsonb_build_object('actor_kind', (SELECT kind::text FROM accounts WHERE id=$3)) \
           || CASE WHEN t.revision_id IS NULL THEN '{}'::jsonb \
                   ELSE jsonb_build_object(CASE WHEN $4='item.delete' THEN 'before_revision_id' ELSE 'after_revision_id' END, t.revision_id) END, \
           $6, s.owner_user_id, $7, $8 \
         FROM spaces s LEFT JOIN text_objects t ON t.space_id=s.id AND t.node_id=$2 \
         WHERE s.id=$1 RETURNING id",
    )
    .bind(event.space_id).bind(event.node_id).bind(event.actor_account_id)
    .bind(event.op_type).bind(metadata).bind(event.operation_id).bind(private).bind(snapshot_id)
    .fetch_one(&mut *tx).await.map_err(map_sqlx_error)?;
    Ok(())
}

fn binding(space: Uuid, id: Uuid) -> String {
    format!("changes/{space}/{id}")
}

/// Only routing identifiers and fixed structural flags are left in clear text.
/// Unknown/new keys are private by default, including names, paths, reasons and sizes.
fn protect_metadata(
    crypto: &PiiCrypto,
    space: Uuid,
    id: Uuid,
    metadata: Value,
) -> Result<(Value, Value)> {
    let mut public = serde_json::Map::new();
    let mut private = serde_json::Map::new();
    let entries = metadata
        .as_object()
        .ok_or_else(|| Error::internal("change metadata must be an object"))?;
    for (key, value) in entries {
        let structural = matches!(
            key.as_str(),
            "item_kind"
                | "source"
                | "actor_kind"
                | "executor_kind"
                | "parent_node_id"
                | "parent_node_id_before"
                | "parent_node_id_after"
                | "copied_from_node_id"
                | "related_deletion_operation_id"
                | "before_revision_id"
                | "after_revision_id"
                | "name_changed"
                | "sort_order_changed"
                | "external_access_enabled_changed"
                | "text_encryption_changed"
                | "write_lock_changed"
                | "external_access_enabled"
                | "text_encryption_enabled"
                | "write_locked"
                | "recursive"
        );
        if structural {
            public.insert(key.clone(), value.clone());
        } else {
            private.insert(key.clone(), value.clone());
        }
    }
    let payload =
        crypto.encrypt_history(&binding(space, id), &Value::Object(private).to_string())?;
    let encrypted =
        serde_json::to_value(payload).map_err(|_| Error::internal("history encoding failed"))?;
    Ok((Value::Object(public), encrypted))
}

fn open_metadata(
    crypto: &PiiCrypto,
    space: Uuid,
    id: Option<Uuid>,
    mut metadata: Value,
    private: Option<Value>,
) -> Result<Value> {
    if let Some(private) = private {
        let id = id.ok_or_else(|| Error::internal("encrypted history has no snapshot identity"))?;
        let encrypted: EncryptedHistoryValue = serde_json::from_value(private)
            .map_err(|_| Error::internal("invalid encrypted change metadata"))?;
        let plaintext = crypto.decrypt_history(&binding(space, id), &encrypted)?;
        let values: serde_json::Map<String, Value> = serde_json::from_str(&plaintext)
            .map_err(|_| Error::internal("invalid change metadata payload"))?;
        metadata
            .as_object_mut()
            .ok_or_else(|| Error::internal("invalid change metadata"))?
            .extend(values);
    }
    Ok(metadata)
}

/// Encrypt legacy rows in short transactions; no plaintext snapshot is copied elsewhere.
pub(crate) async fn encrypt_legacy(pool: &PgPool, crypto: &PiiCrypto) -> Result<u64> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    let rows: Vec<(i64, Uuid, Value)> = sqlx::query_as(
        "SELECT id, space_id, metadata FROM file_change_events WHERE private_metadata IS NULL \
         ORDER BY id LIMIT 100 FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let count = rows.len() as u64;
    for (id, space, metadata) in rows {
        let snapshot_id = Uuid::new_v4();
        let (metadata, private) = protect_metadata(crypto, space, snapshot_id, metadata)?;
        sqlx::query("UPDATE file_change_events e SET metadata=$2, private_metadata=$3, snapshot_id=$4, owner_user_id=COALESCE(owner_user_id, (SELECT s.owner_user_id FROM spaces s WHERE s.id=e.space_id)) WHERE id=$1")
            .bind(id)
            .bind(metadata)
            .bind(private)
            .bind(snapshot_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_error)?;
    }
    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(count)
}

/// List file-change events for `space_id` (optionally scoped to `node_id`),
/// newest first by display time using the event time indexes.
pub(crate) async fn list_file_change_events(
    pool: &PgPool,
    crypto: &PiiCrypto,
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

    with_revision_availability(pool, decode_rows(crypto, rows)?).await
}

/// List the same event rows before an MCP changes cursor by the canonical
/// Space-local mutation sequence.
pub(crate) async fn list_file_change_events_by_id(
    pool: &PgPool,
    crypto: &PiiCrypto,
    space_id: Uuid,
    limit: i64,
    before_id: Option<i64>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    let rows = list_event_rows_by_id::<FileChangeEventRow>(
        pool,
        "file_change_events",
        FILE_CHANGE_EVENT_COLUMNS,
        UuidFilter::new("space_id", space_id),
        limit,
        before_id,
    )
    .await?;

    with_revision_availability(pool, decode_rows(crypto, rows)?).await
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
    crypto: &PiiCrypto,
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

    let rows = sqlx::query_as::<_, FileChangeEventRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {FILE_CHANGE_EVENT_COLUMNS} FROM file_change_events \
         WHERE space_id = $1 AND id > $2 \
         ORDER BY id ASC LIMIT $3"
    )))
    .bind(space_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(FileChangeSyncRows {
        events: decode_rows(crypto, rows)?,
        latest_id,
        token_valid: true,
    })
}

#[derive(Debug, FromRow)]
struct FileChangeEventRow {
    id: i64,
    operation_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    space_id: Uuid,
    node_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    op_type: String,
    metadata: Value,
    private_metadata: Option<Value>,
    snapshot_id: Option<Uuid>,
}

fn decode_rows(
    crypto: &PiiCrypto,
    rows: Vec<FileChangeEventRow>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    rows.into_iter()
        .map(|row| {
            Ok(notegate_model::FileChangeEvent {
                id: row.id,
                operation_id: row.operation_id,
                created_at: row.created_at,
                space_id: row.space_id,
                node_id: row.node_id,
                actor_account_id: row.actor_account_id,
                op_type: row.op_type,
                metadata: open_metadata(
                    crypto,
                    row.space_id,
                    row.snapshot_id,
                    row.metadata,
                    row.private_metadata,
                )?,
            })
        })
        .collect()
}

#[derive(FromRow)]
struct RevisionPresence {
    id: Uuid,
    space_id: Uuid,
    node_id: Uuid,
    state: String,
    cleanup_at: Option<DateTime<Utc>>,
}

async fn with_revision_availability(
    pool: &PgPool,
    mut events: Vec<notegate_model::FileChangeEvent>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    // One bounded batch lookup, never a body read or per-event query. A reference
    // survives body deletion in Changes; missing bodies are explicitly unavailable.
    let ids: Vec<Uuid> = events
        .iter()
        .flat_map(|event| {
            ["before_revision_id", "after_revision_id"]
                .into_iter()
                .filter_map(|key| event.metadata.get(key)?.as_str()?.parse().ok())
        })
        .collect();
    if !ids.is_empty() {
        let statuses: Vec<RevisionPresence> = sqlx::query_as(
            "SELECT revision_id AS id, space_id, node_id, 'current'::text AS state, NULL::timestamptz AS cleanup_at FROM text_objects \
             WHERE revision_id = ANY($1) UNION ALL \
             SELECT id, space_id, node_id, 'retained'::text, cleanup_at FROM text_revisions WHERE id = ANY($1)",
        ).bind(&ids).fetch_all(pool).await.map_err(map_sqlx_error)?;
        let statuses: std::collections::HashMap<_, _> = statuses
            .into_iter()
            .map(|row| {
                (
                    (row.id, row.space_id, row.node_id),
                    (row.state, row.cleanup_at),
                )
            })
            .collect();
        for event in &mut events {
            for side in ["before", "after"] {
                let Some(id) = event
                    .metadata
                    .get(format!("{side}_revision_id"))
                    .and_then(Value::as_str)
                    .and_then(|id| id.parse::<Uuid>().ok())
                else {
                    continue;
                };
                let status = event
                    .node_id
                    .and_then(|node| statuses.get(&(id, event.space_id, node)));
                let metadata = event
                    .metadata
                    .as_object_mut()
                    .ok_or_else(|| Error::internal("change metadata must be an object"))?;
                metadata.insert(
                    format!("{side}_revision_status"),
                    serde_json::json!(status.map_or("unavailable", |r| r.0.as_str())),
                );
                metadata.insert(
                    format!("{side}_revision_cleanup_at"),
                    serde_json::json!(status.and_then(|r| r.1)),
                );
            }
        }
    }
    Ok(events)
}

pub(crate) async fn list_by_owner(
    pool: &PgPool,
    crypto: &PiiCrypto,
    owner: Uuid,
    space: Option<Uuid>,
    limit: i64,
    cursor: Option<&FileChangeEventCursor>,
) -> Result<Vec<notegate_model::FileChangeEvent>> {
    let rows = list_event_rows::<FileChangeEventRow>(
        pool,
        "file_change_events",
        FILE_CHANGE_EVENT_COLUMNS,
        UuidFilter::new("owner_user_id", owner),
        space.map(|id| UuidFilter::new("space_id", id)),
        limit,
        cursor.map(|c| EventCursorPosition {
            created_at: c.created_at,
            id: c.id,
        }),
    )
    .await?;
    with_revision_availability(pool, decode_rows(crypto, rows)?).await
}

const FILE_CHANGE_EVENT_COLUMNS: &str = "id, operation_id, created_at, space_id, node_id, actor_account_id, op_type, metadata, private_metadata, snapshot_id";
