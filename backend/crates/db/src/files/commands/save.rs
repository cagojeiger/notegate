//! Save command: replace a text's content + metrics (`write`/`patch`).
//!
//! Runs in one transaction: re-reads the text's current byte length, enforces
//! the space byte budget for the replacement, updates `text_objects` content +
//! metrics + attribution, and bumps the node's `updated_by`/`updated_at`.

use notegate_core::limits::Limits;
use notegate_core::security::PiiCrypto;
use notegate_core::{Error, Result};
use notegate_model::files::StoredContent;
use notegate_model::{Node, TextObject};
use sqlx::PgPool;
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
use super::{checks, stored_text_parts};
use crate::file_change_events;
use crate::files_repo::TextMutationKind;
use crate::space_usage::{self, UsageDelta};

pub struct SaveTextContentArgs<'a> {
    pub pool: &'a PgPool,
    pub crypto: &'a PiiCrypto,
    pub space_id: Uuid,
    pub node_id: Uuid,
    pub content: &'a StoredContent,
    pub expected_sha256: Option<&'a str>,
    pub updated_by: Uuid,
    pub mutation_kind: TextMutationKind,
    pub caps: Limits,
}

/// Replace a live text's content + metrics, attributing the update to
/// `updated_by` on both the text and its node.
pub async fn save_text_content(args: SaveTextContentArgs<'_>) -> Result<(Node, TextObject)> {
    let SaveTextContentArgs {
        pool,
        crypto,
        space_id,
        node_id,
        content,
        expected_sha256,
        updated_by,
        mutation_kind,
        caps,
    } = args;

    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;

    // Current byte length/hash (for budget delta + optimistic guard); the
    // text row is locked so `expected_sha256` is compared atomically with
    // the following update.
    let node_row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {NODE_COLUMNS} FROM nodes \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
         FOR UPDATE"
    )))
    .bind(space_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("text not found"))?;

    let current_text = sqlx::query_as::<_, TextRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {TEXT_COLUMNS} FROM text_objects \
         WHERE space_id = $1 AND node_id = $2 \
         FOR UPDATE"
    )))
    .bind(space_id)
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("text not found"))?;
    if let Some(expected) = expected_sha256
        && expected != current_text.content_sha256
    {
        return Err(Error::conflict(
            "expected_sha256 does not match the current text; read it again",
        ));
    }

    let encrypt_at_rest = current_text.at_rest_encryption == "server";
    let target_at_rest = match &content.body {
        notegate_model::files::WriteTextBody::Plain(_) if encrypt_at_rest => "server",
        _ => "none",
    };
    let content_changed = current_text.storage_format
        != match &content.body {
            notegate_model::files::WriteTextBody::Plain(_) => "plain",
            notegate_model::files::WriteTextBody::Encrypted(_) => "encrypted",
        }
        || current_text.at_rest_encryption != target_at_rest
        || match &content.body {
            notegate_model::files::WriteTextBody::Plain(value)
                if current_text.at_rest_encryption == "none" =>
            {
                current_text.content.as_deref() != Some(value.as_str())
            }
            notegate_model::files::WriteTextBody::Plain(_) => false,
            notegate_model::files::WriteTextBody::Encrypted(payload) => {
                current_text.encrypted_payload.as_ref() != Some(payload)
            }
        }
        || current_text.content_sha256 != content.content_sha256
        || current_text.byte_len != content.byte_len
        || current_text.line_count != content.line_count;
    if !content_changed {
        tx.commit().await.map_err(map_sqlx_error)?;
        return Ok((node_row.into_node()?, current_text.into_text(crypto)?));
    }
    checks::require_node_write(&mut tx, space_id, node_id).await?;

    space_usage::apply_quota_delta(
        &mut tx,
        &locked.gate,
        UsageDelta::text(0, content.byte_len - current_text.byte_len),
        locked.limits,
    )
    .await?;

    let stored = stored_text_parts(
        content,
        encrypt_at_rest,
        locked.owner_tier,
        crypto,
        space_id,
        node_id,
    )?;
    let doc_row = sqlx::query_as::<_, TextRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE text_objects \
         SET storage_format = $3, content_text = $4, encrypted_payload = $5, \
             content_sha256 = $6, byte_len = $7, line_count = $8, \
             at_rest_encryption = $9, content_ciphertext = $10, content_nonce = $11, \
             content_enc_key_id = $12, content_enc_version = $13, \
             updated_by_account_id = $14, updated_at = now() \
         WHERE space_id = $1 AND node_id = $2 RETURNING {TEXT_COLUMNS}"
    )))
    .bind(space_id)
    .bind(node_id)
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
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_constraint_error)?;

    let node_row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE nodes SET updated_by_account_id = $3, updated_at = now() \
         WHERE space_id = $1 AND id = $2 RETURNING {NODE_COLUMNS}"
    )))
    .bind(space_id)
    .bind(node_id)
    .bind(updated_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    file_change_events::text_saved(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        node_id,
        &node_row.name,
        node_row.parent_id,
        mutation_kind,
        current_text.byte_len,
        content.byte_len,
        current_text.line_count,
        content.line_count,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((node_row.into_node()?, doc_row.into_text(crypto)?))
}
