//! Server-side copy command (`cp`).
//!
//! Copies a live node inside one space-serialized transaction. The new root
//! gets a fresh id/location/attribution, while node metadata and text/file
//! content payloads are preserved.

use std::collections::HashMap;

use notegate_core::limits::{self, Limits};
use notegate_core::{Error, Result};
use notegate_model::files::CopyCounts;
use notegate_model::{FileStorageKind, Node};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use super::super::error::{map_constraint_error, map_sqlx_error};
use super::super::rows::{FILE_COLUMNS, FileRow, NODE_COLUMNS, NodeRow, TEXT_COLUMNS, TextRow};
use super::checks;
use crate::file_change_events;
use crate::space_usage::{self, UsageDelta};

pub struct CopyNodeArgs<'a> {
    pub pool: &'a PgPool,
    pub space_id: Uuid,
    pub source_node_id: Uuid,
    pub new_parent_id: Uuid,
    pub new_name: &'a str,
    pub recursive: bool,
    pub created_by: Uuid,
    pub caps: Limits,
}

pub async fn copy_node(args: CopyNodeArgs<'_>) -> Result<(Node, CopyCounts)> {
    let CopyNodeArgs {
        pool,
        space_id,
        source_node_id,
        new_parent_id,
        new_name,
        recursive,
        created_by,
        caps,
    } = args;
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;
    checks::lock_space(&mut tx, space_id).await?;
    let caps = checks::effective_limits_for_locked_space(&mut tx, space_id, caps).await?;

    let source = checks::live_node(&mut tx, space_id, source_node_id)
        .await?
        .ok_or_else(|| Error::not_found("source node not found"))?;
    if source.parent_id.is_none() {
        return Err(Error::conflict("cannot copy the root node"));
    }
    if source.kind == "folder" && !recursive {
        return Err(Error::conflict("folder copy requires recursive=true"));
    }
    let source_kind = source.kind.clone();

    checks::require_live_folder(&mut tx, space_id, new_parent_id).await?;
    checks::require_sibling_unique(&mut tx, space_id, new_parent_id, new_name, None).await?;
    checks::require_fanout(&mut tx, space_id, new_parent_id, caps).await?;

    let snapshot = load_subtree(&mut tx, space_id, source_node_id).await?;
    if snapshot.len() > limits::SUBTREE_DELETE_MAX_NODES {
        return Err(Error::conflict(format!(
            "subtree of {} nodes exceeds the synchronous copy limit of {}",
            snapshot.len(),
            limits::SUBTREE_DELETE_MAX_NODES
        )));
    }

    let source_depth = snapshot.iter().map(|node| node.depth).max().unwrap_or(0) as usize;
    let dest_depth = checks::node_depth(&mut tx, space_id, new_parent_id).await?;
    if dest_depth + 1 + source_depth > limits::MAX_PATH_DEPTH {
        return Err(Error::conflict(format!(
            "copy would exceed the maximum path depth of {}",
            limits::MAX_PATH_DEPTH
        )));
    }

    let counts = CopyCounts {
        nodes: snapshot.len(),
        texts: snapshot.iter().filter(|node| node.kind == "text").count(),
        files: snapshot.iter().filter(|node| node.kind == "file").count(),
    };
    let content_bytes = copied_content_bytes(&mut tx, space_id, source_node_id).await?;
    let copied_nodes = i64::try_from(counts.nodes)
        .map_err(|_error| Error::internal("copied node count exceeds bigint"))?;
    space_usage::apply_quota_delta(
        &mut tx,
        space_id,
        UsageDelta::new(copied_nodes, content_bytes),
        caps,
    )
    .await?;

    let mut id_map = HashMap::with_capacity(snapshot.len());
    let mut copied_root = None;
    for source in snapshot {
        let new_parent = if source.id == source_node_id {
            new_parent_id
        } else {
            let Some(parent_id) = source.parent_id.and_then(|id| id_map.get(&id).copied()) else {
                return Err(Error::internal(
                    "copy traversal produced child before parent",
                ));
            };
            parent_id
        };
        let name = if source.id == source_node_id {
            new_name
        } else {
            &source.name
        };
        let node =
            insert_copied_node(&mut tx, space_id, new_parent, name, &source, created_by).await?;
        id_map.insert(source.id, node.id);
        copy_content(&mut tx, space_id, source.id, &node, created_by).await?;
        if source.id == source_node_id {
            copied_root = Some(node);
        }
    }
    let copied_root = copied_root.ok_or_else(|| Error::internal("copy produced no root node"))?;

    file_change_events::node_copied(
        &mut tx,
        file_change_events::context(created_by, space_id),
        copied_root.id,
        &source_kind,
        source_node_id,
        new_parent_id,
        counts,
        recursive,
    )
    .await?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok((copied_root, counts))
}

#[derive(Debug, FromRow)]
struct CopyNodeRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    sort_order: i32,
    metadata: Value,
    depth: i32,
}

async fn load_subtree(
    tx: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
) -> Result<Vec<CopyNodeRow>> {
    sqlx::query_as(
        "WITH RECURSIVE subtree AS ( \
                SELECT id, parent_id, name, kind, sort_order, metadata, 0 AS depth \
                FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id, n.parent_id, n.name, n.kind, n.sort_order, n.metadata, s.depth + 1 \
                FROM nodes n JOIN subtree s ON n.parent_id = s.id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
             ) \
             SELECT id, parent_id, name, kind, sort_order, metadata, depth \
             FROM subtree ORDER BY depth, sort_order, name, id",
    )
    .bind(space_id)
    .bind(source_node_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(map_sqlx_error)
}

async fn copied_content_bytes(
    tx: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
) -> Result<i64> {
    sqlx::query_scalar(
            "WITH RECURSIVE subtree AS ( \
                SELECT id FROM nodes WHERE space_id = $1 AND id = $2 AND deleted_at IS NULL \
                UNION ALL \
                SELECT n.id FROM nodes n JOIN subtree s ON n.parent_id = s.id \
                WHERE n.space_id = $1 AND n.deleted_at IS NULL \
             ) \
             SELECT \
                COALESCE((SELECT sum(t.byte_len) FROM text_objects t JOIN subtree s ON s.id = t.node_id WHERE t.space_id = $1), 0)::bigint + \
                COALESCE((SELECT sum(f.byte_len) FROM file_objects f JOIN subtree s ON s.id = f.node_id WHERE f.space_id = $1), 0)::bigint",
        )
        .bind(space_id)
        .bind(source_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_error)
}

async fn insert_copied_node(
    tx: &mut PgConnection,
    space_id: Uuid,
    parent_id: Uuid,
    name: &str,
    source: &CopyNodeRow,
    created_by: Uuid,
) -> Result<Node> {
    let row = sqlx::query_as::<_, NodeRow>(&format!(
            "INSERT INTO nodes \
             (space_id, parent_id, name, kind, sort_order, metadata, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7) RETURNING {NODE_COLUMNS}"
        ))
        .bind(space_id)
        .bind(parent_id)
        .bind(name)
        .bind(&source.kind)
        .bind(source.sort_order)
        .bind(&source.metadata)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
    row.into_node()
}

async fn copy_content(
    tx: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
    new_node: &Node,
    created_by: Uuid,
) -> Result<()> {
    match new_node.kind.as_str() {
        "folder" => Ok(()),
        "text" => copy_text(tx, space_id, source_node_id, new_node.id, created_by).await,
        "file" => copy_file(tx, space_id, source_node_id, new_node.id).await,
        _ => Err(Error::internal("unknown node kind during copy")),
    }
}

async fn copy_text(
    tx: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
    new_node_id: Uuid,
    created_by: Uuid,
) -> Result<()> {
    let source = sqlx::query_as::<_, TextRow>(&format!(
        "SELECT {TEXT_COLUMNS} FROM text_objects WHERE space_id = $1 AND node_id = $2"
    ))
    .bind(space_id)
    .bind(source_node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    sqlx::query(&format!(
            "INSERT INTO text_objects \
             (node_id, space_id, storage_format, content_text, encrypted_payload, content_sha256, byte_len, line_count, \
              media_type, encoding, created_by_account_id, updated_by_account_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11) RETURNING {TEXT_COLUMNS}"
        ))
        .bind(new_node_id)
        .bind(space_id)
        .bind(source.storage_format)
        .bind(source.content)
        .bind(source.encrypted_payload)
        .bind(source.content_sha256)
        .bind(source.byte_len)
        .bind(source.line_count)
        .bind(source.media_type)
        .bind(source.encoding)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
    Ok(())
}

async fn copy_file(
    tx: &mut PgConnection,
    space_id: Uuid,
    source_node_id: Uuid,
    new_node_id: Uuid,
) -> Result<()> {
    let source = sqlx::query_as::<_, FileRow>(&format!(
        "SELECT {FILE_COLUMNS} FROM file_objects WHERE space_id = $1 AND node_id = $2"
    ))
    .bind(space_id)
    .bind(source_node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;
    let file = source.into_file()?;
    if file.storage_kind != FileStorageKind::InlinePg {
        return Err(Error::conflict(
            "copy does not support object-storage files yet",
        ));
    }
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT bytes FROM file_inline_contents WHERE space_id = $1 AND node_id = $2",
    )
    .bind(space_id)
    .bind(source_node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    sqlx::query(&format!(
            "INSERT INTO file_objects \
             (node_id, space_id, storage_kind, media_type, byte_len, content_sha256, original_filename, encryption_mode, encryption_metadata) \
             VALUES ($1, $2, 'inline_pg', $3, $4, $5, $6, $7, $8) RETURNING {FILE_COLUMNS}"
        ))
        .bind(new_node_id)
        .bind(space_id)
        .bind(file.media_type)
        .bind(file.byte_len)
        .bind(file.content_sha256)
        .bind(file.original_filename)
        .bind(file.encryption_mode.as_str())
        .bind(file.encryption_metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_constraint_error)?;

    sqlx::query("INSERT INTO file_inline_contents (node_id, space_id, bytes) VALUES ($1, $2, $3)")
        .bind(new_node_id)
        .bind(space_id)
        .bind(bytes)
        .execute(&mut *tx)
        .await
        .map_err(map_constraint_error)?;
    Ok(())
}
