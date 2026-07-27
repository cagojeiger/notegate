//! Create commands: `mkdir` (folder) and `touch`/`write-create` (text).
//!
//! Both run in one transaction that re-checks every create invariant — parent is
//! a live folder, resulting path/fanout/node caps, sibling-name unique, and
//! shared content byte budget — then inserts the node (and content row) with
//! attribution = the caller.

use notegate_core::Result;
use notegate_core::limits::Limits;
use notegate_core::security::PiiCrypto;
use notegate_model::files::StoredContent;
use notegate_model::{Node, TextObject};
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
use super::{checks, stored_text_parts};
use crate::file_change_events;
use crate::space_usage::{self, UsageDelta};

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

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;
    prepare_create(&mut tx, space_id, parent_id, name, locked.limits).await?;
    space_usage::apply_quota_delta(&mut tx, &locked.gate, UsageDelta::nodes(1), locked.limits)
        .await?;

    let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, search_enabled, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'folder', $4, $5, $5) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(locked.default_search_enabled)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::folder_created(
        &mut tx,
        file_change_events::context(created_by, space_id),
        row.id,
        &row.name,
        parent_id,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

/// Insert a text node + its `text_objects` row, attributing both to
/// `created_by`. `content` carries the pre-computed metrics from the service.
pub struct InsertTextArgs<'a> {
    pub pool: &'a PgPool,
    pub crypto: &'a PiiCrypto,
    pub space_id: Uuid,
    pub parent_id: Uuid,
    pub name: &'a str,
    pub content: &'a StoredContent,
    pub created_by: Uuid,
    pub caps: Limits,
}

pub async fn insert_text(args: InsertTextArgs<'_>) -> Result<(Node, TextObject)> {
    let InsertTextArgs {
        pool,
        crypto,
        space_id,
        parent_id,
        name,
        content,
        created_by,
        caps,
    } = args;
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;
    prepare_create(&mut tx, space_id, parent_id, name, locked.limits).await?;
    space_usage::apply_quota_delta(
        &mut tx,
        &locked.gate,
        UsageDelta::text(1, content.byte_len),
        locked.limits,
    )
    .await?;

    let node_row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes (space_id, parent_id, name, kind, search_enabled, created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, 'text', $4, $5, $5) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(locked.default_search_enabled)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    let stored = stored_text_parts(
        content,
        locked.default_text_encryption_enabled,
        locked.owner_tier,
        crypto,
        space_id,
        node_row.id,
    )?;
    let doc_row = sqlx::query_as::<_, TextRow>(&format!(
            "INSERT INTO text_objects \
            (node_id, space_id, storage_format, content_text, encrypted_payload, content_sha256, byte_len, line_count, \
             at_rest_encryption, content_ciphertext, content_nonce, content_enc_key_id, content_enc_version, \
             created_by_account_id, updated_by_account_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $14) \
         RETURNING {TEXT_COLUMNS}"
        ))
        .bind(node_row.id)
        .bind(space_id)
        .bind(stored.storage_format)
        .bind(stored.content_text)
        .bind(stored.encrypted_payload)
        .bind(&content.content_sha256)
        .bind(content.byte_len)
        .bind(content.line_count)
        .bind(stored.at_rest_encryption)
        .bind(stored.content_ciphertext)
        .bind(stored.content_nonce)
        .bind(stored.content_enc_key_id)
        .bind(stored.content_enc_version)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    file_change_events::text_created(
        &mut tx,
        file_change_events::context(created_by, space_id),
        node_row.id,
        &node_row.name,
        parent_id,
        content.byte_len,
        content.line_count,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, doc_row.into_text(crypto)?))
}

/// Shared in-tx create pre-checks: parent live folder, path bounds,
/// sibling-unique, and fanout.
pub(crate) async fn prepare_create(
    tx: &mut sqlx::PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    caps: Limits,
) -> Result<()> {
    let parent_bounds =
        checks::require_writable_folder_path_bounds(tx, space_id, parent_id).await?;
    let bounds = checks::destination_bounds(parent_bounds, name, checks::PathBounds::default())?;
    checks::require_path_limits(bounds)?;

    checks::require_sibling_unique(tx, space_id, parent_id, name, None).await?;
    checks::require_fanout(tx, space_id, parent_id, caps).await?;
    Ok(())
}
