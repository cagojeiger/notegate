//! Create commands: `mkdir` (folder) and `touch`/`write-create` (text).
//!
//! Both run in one transaction that re-checks every create invariant — parent is
//! a live folder, resulting depth/fanout/node caps, sibling-name unique, and
//! shared content byte budget — then inserts the node (and content row) with
//! attribution = the caller.

use notegate_core::limits::{self, Limits};
use notegate_core::{Error, Result};
use notegate_model::files::{StoredContent, StoredFile};
use notegate_model::{FileObject, Node, TextObject};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
use super::{checks, stored_text_parts};
use crate::file_change_events;

/// Insert a folder under `parent_id`, attributing it to `created_by`.
pub async fn insert_folder(
    pool: &PgPool,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    created_by: Uuid,
    caps: Limits,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let caps = checks::effective_limits_for_locked_space(&mut tx, space_id, caps).await?;
    prepare_create(&mut tx, space_id, parent_id, name, caps).await?;

    let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'folder', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::record(
        &mut tx,
        file_change_events::context(created_by, space_id),
        Some(row.id),
        "folder.create",
        json!({
            "item_kind": "folder",
            "parent_node_id": parent_id,
        }),
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

/// Insert a text node + its `text_objects` row, attributing both to
/// `created_by`. `content` carries the pre-computed metrics from the service.
pub async fn insert_text(
    pool: &PgPool,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    content: &StoredContent,
    created_by: Uuid,
    caps: Limits,
) -> Result<(Node, TextObject)> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let caps = checks::effective_limits_for_locked_space(&mut tx, space_id, caps).await?;
    prepare_create(&mut tx, space_id, parent_id, name, caps).await?;
    checks::require_content_budget(&mut tx, space_id, 0, content.byte_len, caps).await?;

    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'text', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    let (storage_format, content_text, encrypted_payload) = stored_text_parts(content);
    let doc_row = sqlx::query_as::<_, TextRow>(&format!(
            "INSERT INTO text_objects \
            (node_id, space_id, storage_format, content_text, encrypted_payload, content_sha256, byte_len, line_count, \
             created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9) RETURNING {TEXT_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(space_id)
        .bind(storage_format)
        .bind(content_text)
        .bind(encrypted_payload)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::record(
        &mut tx,
        file_change_events::context(created_by, space_id),
        Some(node_row.id),
        "text.create",
        json!({
            "item_kind": "text",
            "parent_node_id": parent_id,
            "byte_len_after": content.byte_len,
            "line_count_after": content.line_count,
        }),
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, doc_row.into_text()?))
}

/// Insert a file node + metadata and inline bytes, attributing it to `created_by`.
pub async fn insert_file(
    pool: &PgPool,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    file: &StoredFile,
    created_by: Uuid,
    caps: Limits,
) -> Result<(Node, FileObject)> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let caps = checks::effective_limits_for_locked_space(&mut tx, space_id, caps).await?;
    prepare_create(&mut tx, space_id, parent_id, name, caps).await?;
    checks::require_content_budget(&mut tx, space_id, 0, file.byte_len, caps).await?;

    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'file', $4, $4) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    let file_row = sqlx::query_as::<_, FileRow>(&format!(
            "INSERT INTO file_objects \
            (node_id, space_id, storage_kind, media_type, byte_len, content_sha256, original_filename, encryption_mode, encryption_metadata) \
         VALUES ($1, $2, 'inline_pg', $3, $4, $5, $6, $7, $8) RETURNING {FILE_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(space_id)
        .bind(&file.media_type)
        .bind(file.byte_len)
        .bind(&file.content_sha256)
        .bind(&file.original_filename)
        .bind(file.encryption_mode.as_str())
        .bind(&file.encryption_metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    sqlx::query("INSERT INTO file_inline_contents (node_id, space_id, bytes) VALUES ($1, $2, $3)")
        .bind(node_row.id)
        .bind(space_id)
        .bind(&file.bytes)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::record(
        &mut tx,
        file_change_events::context(created_by, space_id),
        Some(node_row.id),
        "file.create",
        json!({
            "item_kind": "file",
            "parent_node_id": parent_id,
            "byte_len_after": file.byte_len,
        }),
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, file_row.into_file()?))
}

/// Shared in-tx create pre-checks: parent live folder, depth, sibling-unique,
/// fanout, and space node budget.
async fn prepare_create(
    tx: &mut sqlx::PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    caps: Limits,
) -> Result<()> {
    checks::require_live_folder(tx, space_id, parent_id).await?;

    let parent_depth = checks::node_depth(tx, space_id, parent_id).await?;
    if parent_depth + 1 > limits::MAX_PATH_DEPTH {
        return Err(Error::validation(format!(
            "path depth would exceed the maximum of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    checks::require_sibling_unique(tx, space_id, parent_id, name, None).await?;
    checks::require_fanout(tx, space_id, parent_id, caps).await?;
    checks::require_node_budget(tx, space_id, caps).await?;
    Ok(())
}
