//! Row types for the file tree (`nodes`) and document content (`documents`),
//! plus the shared column lists. There is no stored `path` column — the display
//! path is derived via a recursive CTE (see `queries`).

use chrono::{DateTime, Utc};
use notegate_core::{Error, Result};
use notegate_model::{Document, Node, NodeKind};
use sqlx::FromRow;
use uuid::Uuid;

/// A row from `nodes`.
#[derive(Debug, FromRow)]
pub struct NodeRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub sort_order: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub deleted_by: Option<Uuid>,
    pub purge_after: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl NodeRow {
    /// Convert into the domain [`Node`], parsing the kind.
    pub fn into_node(self) -> Result<Node> {
        let kind = NodeKind::parse(&self.kind)
            .ok_or_else(|| Error::internal(format!("unknown node kind: {}", self.kind)))?;
        Ok(Node {
            id: self.id,
            workspace_id: self.workspace_id,
            parent_id: self.parent_id,
            name: self.name,
            kind,
            sort_order: self.sort_order,
            created_by: self.created_by,
            updated_by: self.updated_by,
            deleted_by: self.deleted_by,
            purge_after: self.purge_after,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
        })
    }
}

/// A row from `documents`.
#[derive(Debug, FromRow)]
pub struct DocumentRow {
    pub node_id: Uuid,
    pub workspace_id: Uuid,
    pub content_md: String,
    pub content_sha256: String,
    pub byte_len: i32,
    pub line_count: i32,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DocumentRow> for Document {
    fn from(row: DocumentRow) -> Self {
        Self {
            node_id: row.node_id,
            workspace_id: row.workspace_id,
            content_md: row.content_md,
            content_sha256: row.content_sha256,
            byte_len: row.byte_len,
            line_count: row.line_count,
            created_by: row.created_by,
            updated_by: row.updated_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Selectable columns of `nodes`, in [`NodeRow`] order.
pub const NODE_COLUMNS: &str = "id, workspace_id, parent_id, name, kind, sort_order, \
     created_by, updated_by, deleted_by, purge_after, created_at, updated_at, deleted_at";

/// Selectable columns of `documents`, in [`DocumentRow`] order.
pub const DOCUMENT_COLUMNS: &str = "node_id, workspace_id, content_md, content_sha256, \
     byte_len, line_count, created_by, updated_by, created_at, updated_at";
