//! Node property and policy updates.
//!
//! Property updates rename or reorder a node. Policy updates change search
//! visibility or Text encryption, rewriting encrypted content in the same
//! transaction.

use notegate_core::security::PiiCrypto;
use notegate_core::{Error, Result};
use notegate_model::Node;
use notegate_model::files::{
    StoredContent, UpdateNode, UpdateNodeSearchPolicy, UpdateTextEncryption, WriteTextBody,
};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
use super::{checks, lock_live_node, stored_text_parts};
use crate::file_change_events;

pub async fn update_node(
    pool: &PgPool,
    space_id: Uuid,
    command: &UpdateNode,
    updated_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;

    let current = lock_live_node(&mut tx, space_id, command.node_id).await?;
    let node_kind = current.kind.clone();
    let rename = match (command.name.as_deref(), current.parent_id) {
        (Some(_), None) => return Err(Error::conflict("cannot rename the root node")),
        (Some(name), Some(parent_id)) => Some((name, parent_id)),
        (None, _) => None,
    };

    let name_changed = rename.is_some_and(|(name, _)| name != current.name);
    let sort_order_changed = command
        .sort_order
        .is_some_and(|sort_order| sort_order != current.sort_order);
    if !name_changed && !sort_order_changed {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }
    if let Some((name, parent_id)) = rename
        && name_changed
    {
        let access = checks::require_move_write(
            &mut tx,
            space_id,
            command.node_id,
            parent_id,
            &current.kind,
        )
        .await?;
        checks::require_sibling_unique(&mut tx, space_id, parent_id, name, Some(command.node_id))
            .await?;
        let bounds = checks::destination_bounds(access.destination, name, access.subtree)?;
        checks::require_path_limits(bounds)?;
    } else {
        checks::require_node_write(&mut tx, space_id, command.node_id).await?;
    }

    let row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE nodes \
         SET name = COALESCE($3, name), \
             sort_order = COALESCE($4, sort_order), \
             updated_by_account_id = $5, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    )))
    .bind(space_id)
    .bind(command.node_id)
    .bind(command.name.as_deref())
    .bind(command.sort_order)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_constraint_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    file_change_events::node_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        command.node_id,
        file_change_events::NodeUpdated {
            item_kind: &node_kind,
            item_name: &row.name,
            parent_node_id: row.parent_id,
            name_changed,
            sort_order_changed,
            search_enabled_changed: false,
            text_encryption_changed: false,
            search_enabled: row.search_enabled,
            text_encryption_enabled: None,
        },
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

pub async fn update_node_search_policy(
    pool: &PgPool,
    space_id: Uuid,
    command: &UpdateNodeSearchPolicy,
    updated_by: Uuid,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    checks::lock_space(&mut tx, space_id).await?;
    let current = lock_live_node(&mut tx, space_id, command.node_id).await?;

    if current.parent_id.is_none() {
        return Err(Error::conflict(
            "search policy cannot be changed on the root node",
        ));
    }
    if command.enabled == current.search_enabled {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }
    checks::require_node_write(&mut tx, space_id, command.node_id).await?;

    let row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE nodes \
         SET search_enabled = $3, updated_by_account_id = $4, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    )))
    .bind(space_id)
    .bind(command.node_id)
    .bind(command.enabled)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    file_change_events::node_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        command.node_id,
        file_change_events::NodeUpdated {
            item_kind: &row.kind,
            item_name: &row.name,
            parent_node_id: row.parent_id,
            name_changed: false,
            sort_order_changed: false,
            search_enabled_changed: true,
            text_encryption_changed: false,
            search_enabled: row.search_enabled,
            text_encryption_enabled: None,
        },
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

pub async fn update_text_encryption(
    pool: &PgPool,
    crypto: &PiiCrypto,
    space_id: Uuid,
    command: &UpdateTextEncryption,
    updated_by: Uuid,
    caps: notegate_core::limits::Limits,
) -> Result<Node> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let locked = checks::lock_space_context(&mut tx, space_id, caps).await?;
    let current = lock_live_node(&mut tx, space_id, command.node_id).await?;
    if current.kind != "text" {
        return Err(Error::validation(
            "text encryption applies only to text nodes",
        ));
    }

    let current_text = sqlx::query_as::<_, TextRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {TEXT_COLUMNS} FROM text_objects \
         WHERE space_id = $1 AND node_id = $2 FOR UPDATE",
    )))
    .bind(space_id)
    .bind(command.node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| Error::internal("text node has no text object"))?;
    let currently_encrypted = current_text.at_rest_encryption == "server";
    if command.enabled == currently_encrypted {
        tx.commit().await.map_err(map_sqlx_error)?;
        return current.into_node();
    }
    checks::require_node_write(&mut tx, space_id, command.node_id).await?;
    if command.enabled {
        if !locked.owner_tier.features().text_encryption {
            return Err(Error::conflict(
                "text encryption is not available for the space owner's tier",
            ));
        }
        if current_text.storage_format != "plain" {
            return Err(Error::conflict(
                "server text encryption requires plain text storage",
            ));
        }
    }

    let row = sqlx::query_as::<_, NodeRow>(sqlx::AssertSqlSafe(format!(
        "UPDATE nodes \
         SET updated_by_account_id = $3, updated_at = now() \
         WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING {NODE_COLUMNS}"
    )))
    .bind(space_id)
    .bind(command.node_id)
    .bind(updated_by)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_constraint_error)?
    .ok_or_else(|| Error::not_found("node not found"))?;

    rewrite_text_encryption(
        &mut tx,
        current_text,
        command.enabled,
        locked.owner_tier,
        crypto,
        updated_by,
    )
    .await?;

    file_change_events::node_updated(
        &mut tx,
        file_change_events::context(updated_by, space_id),
        command.node_id,
        file_change_events::NodeUpdated {
            item_kind: &row.kind,
            item_name: &row.name,
            parent_node_id: row.parent_id,
            name_changed: false,
            sort_order_changed: false,
            search_enabled_changed: false,
            text_encryption_changed: true,
            search_enabled: row.search_enabled,
            text_encryption_enabled: Some(command.enabled),
        },
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    row.into_node()
}

async fn rewrite_text_encryption(
    tx: &mut Transaction<'_, Postgres>,
    current: TextRow,
    enabled: bool,
    owner_tier: notegate_core::tier::UserTier,
    crypto: &PiiCrypto,
    updated_by: Uuid,
) -> Result<()> {
    let space_id = current.space_id;
    let node_id = current.node_id;
    let text = current.into_text(crypto)?;
    let content = StoredContent {
        body: WriteTextBody::Plain(text.content.ok_or_else(|| {
            Error::conflict("server text encryption requires plain text storage")
        })?),
        content_sha256: text.content_sha256,
        byte_len: text.byte_len,
        line_count: text.line_count,
    };
    let stored = stored_text_parts(&content, enabled, owner_tier, crypto, space_id, node_id)?;

    sqlx::query(
        "UPDATE text_objects \
         SET storage_format = $3, content_text = $4, encrypted_payload = $5, \
             at_rest_encryption = $6, content_ciphertext = $7, \
             content_nonce = $8, content_enc_key_id = $9, content_enc_version = $10, \
             updated_by_account_id = $11, updated_at = now() \
         WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(node_id)
    .bind(stored.storage_format)
    .bind(stored.content_text)
    .bind(stored.encrypted_payload)
    .bind(stored.at_rest_encryption)
    .bind(stored.content_ciphertext)
    .bind(stored.content_nonce)
    .bind(stored.content_enc_key_id)
    .bind(stored.content_enc_version)
    .bind(updated_by)
    .execute(&mut **tx)
    .await
    .map_err(map_sqlx_error)?;
    Ok(())
}
