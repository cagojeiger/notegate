//! Durable coordination between Notegate file nodes and S3-compatible objects.

use notegate_core::limits::{self, Limits};
use notegate_core::{Error, Result};
use notegate_model::files::{BeginObjectUpload, PendingObjectUpload};
use notegate_model::{FileEncryptionMode, FileObject, Node};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use super::commands::{checks, create};
use super::error::{map_constraint_error, map_sqlx_error};
use super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow};
use crate::file_change_events;
use crate::space_usage::{self, UsageDelta};

#[derive(Debug, FromRow)]
struct ObjectUploadRow {
    id: Uuid,
    object_key: String,
    space_id: Option<Uuid>,
    parent_node_id: Option<Uuid>,
    node_id: Option<Uuid>,
    requested_by_account_id: Option<Uuid>,
    name: String,
    declared_byte_len: i64,
    media_type: String,
    original_filename: Option<String>,
    encryption_mode: String,
    encryption_metadata: Option<Value>,
    state: String,
}

impl ObjectUploadRow {
    fn into_pending(self) -> Result<PendingObjectUpload> {
        let encryption_mode = FileEncryptionMode::parse(&self.encryption_mode)
            .ok_or_else(|| Error::internal("unknown object upload encryption mode"))?;
        Ok(PendingObjectUpload {
            id: self.id,
            object_key: self.object_key,
            space_id: self
                .space_id
                .ok_or_else(|| Error::not_found("file upload not found"))?,
            parent_node_id: self
                .parent_node_id
                .ok_or_else(|| Error::not_found("file upload not found"))?,
            requested_by_account_id: self
                .requested_by_account_id
                .ok_or_else(|| Error::not_found("file upload not found"))?,
            name: self.name,
            byte_len: self.declared_byte_len,
            media_type: self.media_type,
            original_filename: self.original_filename,
            encryption_mode,
            encryption_metadata: self.encryption_metadata,
            node_id: self.node_id,
        })
    }
}

const UPLOAD_COLUMNS: &str = "id, object_key, space_id, parent_node_id, node_id, \
    requested_by_account_id, name, declared_byte_len, media_type, original_filename, \
    encryption_mode, encryption_metadata, state";

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    object_key: &str,
    space_id: Uuid,
    requested_by: Uuid,
    input: &BeginObjectUpload,
) -> Result<PendingObjectUpload> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    // Enforce the per-account in-flight upload cap atomically (mirrors
    // `ApiKeyRepo::insert_key_with_cap`): lock the account row so concurrent
    // begins serialize, then re-count `uploading` rows inside the tx. Quota is
    // only charged at attach, so this cap is what bounds unattached staging.
    sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(requested_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_storage_objects \
         WHERE requested_by_account_id = $1 \
           AND state IN ('uploading','expire_pending')",
    )
    .bind(requested_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if pending as usize >= limits::OBJECT_UPLOAD_MAX_PENDING {
        return Err(Error::conflict(format!(
            "too many in-flight uploads; complete or wait for the {} pending uploads to expire",
            limits::OBJECT_UPLOAD_MAX_PENDING
        )));
    }

    let row = sqlx::query_as::<_, ObjectUploadRow>(&format!(
        "INSERT INTO object_storage_objects \
         (id, object_key, space_id, parent_node_id, requested_by_account_id, name, \
          declared_byte_len, media_type, original_filename, encryption_mode, encryption_metadata, state) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'uploading') \
         RETURNING {UPLOAD_COLUMNS}"
    ))
    .bind(id)
    .bind(object_key)
    .bind(space_id)
    .bind(input.parent_node_id)
    .bind(requested_by)
    .bind(&input.name)
    .bind(input.byte_len)
    .bind(&input.media_type)
    .bind(&input.original_filename)
    .bind(input.encryption_mode.as_str())
    .bind(&input.encryption_metadata)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;
    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_pending()
}

pub async fn find(
    pool: &PgPool,
    id: Uuid,
    space_id: Uuid,
    requested_by: Uuid,
) -> Result<Option<PendingObjectUpload>> {
    let row = sqlx::query_as::<_, ObjectUploadRow>(&format!(
        "SELECT {UPLOAD_COLUMNS} FROM object_storage_objects \
         WHERE id = $1 AND space_id = $2 AND requested_by_account_id = $3 \
           AND state IN ('uploading','attached')"
    ))
    .bind(id)
    .bind(space_id)
    .bind(requested_by)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;
    row.map(ObjectUploadRow::into_pending).transpose()
}

pub async fn touch(pool: &PgPool, id: Uuid, space_id: Uuid, requested_by: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE object_storage_objects SET last_activity_at = now(), retry_after = NULL, \
             retry_count = 0, last_error_code = NULL \
         WHERE id = $1 AND space_id = $2 AND requested_by_account_id = $3 \
           AND state = 'uploading'",
    )
    .bind(id)
    .bind(space_id)
    .bind(requested_by)
    .execute(pool)
    .await
    .map_err(map_sqlx_error)?;
    Ok(result.rows_affected() == 1)
}

pub async fn attach(
    pool: &PgPool,
    id: Uuid,
    space_id: Uuid,
    requested_by: Uuid,
    limits: Limits,
) -> Result<(Node, FileObject)> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    let upload = sqlx::query_as::<_, ObjectUploadRow>(&format!(
        "SELECT {UPLOAD_COLUMNS} FROM object_storage_objects \
         WHERE id = $1 AND space_id = $2 AND requested_by_account_id = $3 FOR UPDATE"
    ))
    .bind(id)
    .bind(space_id)
    .bind(requested_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("file upload not found"))?;

    if upload.state == "attached" {
        let node_id = upload
            .node_id
            .ok_or_else(|| Error::internal("attached upload has no node"))?;
        let node = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {NODE_COLUMNS} FROM nodes WHERE id = $1 AND space_id = $2"
        ))
        .bind(node_id)
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        let file = sqlx::query_as::<_, FileRow>(&format!(
            "SELECT {FILE_COLUMNS} FROM file_objects WHERE node_id = $1 AND space_id = $2"
        ))
        .bind(node_id)
        .bind(space_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;
        tx.commit().await.map_err(map_sqlx_error)?;
        return Ok((node.into_node()?, file.into_file()?));
    }
    if upload.state != "uploading" {
        return Err(Error::conflict("file upload is no longer active"));
    }

    let parent_id = upload
        .parent_node_id
        .ok_or_else(|| Error::not_found("upload parent no longer exists"))?;
    let (gate, effective_limits) =
        checks::lock_space_with_limits(&mut tx, space_id, limits).await?;
    create::prepare_create(&mut tx, space_id, parent_id, &upload.name, effective_limits).await?;
    space_usage::apply_quota_delta(
        &mut tx,
        &gate,
        UsageDelta::file(1, upload.declared_byte_len),
        effective_limits,
    )
    .await?;

    let node = sqlx::query_as::<_, NodeRow>(&format!(
        "INSERT INTO nodes \
         (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'file', $4, $4) RETURNING {NODE_COLUMNS}"
    ))
    .bind(space_id)
    .bind(parent_id)
    .bind(&upload.name)
    .bind(requested_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    let file = sqlx::query_as::<_, FileRow>(&format!(
        "INSERT INTO file_objects \
         (node_id, space_id, object_key, media_type, byte_len, original_filename, \
          encryption_mode, encryption_metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING {FILE_COLUMNS}"
    ))
    .bind(node.id)
    .bind(space_id)
    .bind(&upload.object_key)
    .bind(&upload.media_type)
    .bind(upload.declared_byte_len)
    .bind(&upload.original_filename)
    .bind(&upload.encryption_mode)
    .bind(&upload.encryption_metadata)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    file_change_events::file_created(
        &mut tx,
        file_change_events::context(requested_by, space_id),
        node.id,
        &node.name,
        parent_id,
        upload.declared_byte_len,
    )
    .await?;

    let updated = sqlx::query(
        "UPDATE object_storage_objects SET state = 'attached', node_id = $2, attached_at = now(), \
         last_activity_at = now(), last_error_code = NULL \
         WHERE id = $1 AND state = 'uploading'",
    )
    .bind(id)
    .bind(node.id)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::conflict("file upload was completed concurrently"));
    }

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node.into_node()?, file.into_file()?))
}
