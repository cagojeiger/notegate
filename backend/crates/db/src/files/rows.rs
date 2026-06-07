use chrono::{DateTime, Utc};
use uuid::Uuid;

use notegate_domain::files::{Document, DocumentBundle, GrepCandidate, Node, NodeKind};

#[derive(sqlx::FromRow)]
pub(super) struct NodeRow {
    pub(super) id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    path_cache: String,
    sort_order: i32,
    has_children: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl NodeRow {
    pub(super) fn into_node(self) -> Node {
        Node {
            id: self.id,
            parent_id: self.parent_id,
            name: self.name,
            kind: NodeKind::from_storage(&self.kind),
            path: self.path_cache,
            sort_order: self.sort_order,
            has_children: self.has_children,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct DocumentBundleRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    kind: String,
    path_cache: String,
    sort_order: i32,
    has_children: bool,
    node_created_at: DateTime<Utc>,
    node_updated_at: DateTime<Utc>,
    node_id: Uuid,
    workspace_id: Uuid,
    content_md: String,
    content_sha256: String,
    byte_len: i32,
    line_count: i32,
    document_created_at: DateTime<Utc>,
    document_updated_at: DateTime<Utc>,
}

impl DocumentBundleRow {
    pub(super) fn into_bundle(self) -> DocumentBundle {
        DocumentBundle {
            node: Node {
                id: self.id,
                parent_id: self.parent_id,
                name: self.name,
                kind: NodeKind::from_storage(&self.kind),
                path: self.path_cache,
                sort_order: self.sort_order,
                has_children: self.has_children,
                created_at: self.node_created_at,
                updated_at: self.node_updated_at,
            },
            document: Document {
                node_id: self.node_id,
                workspace_id: self.workspace_id,
                content_md: self.content_md,
                content_sha256: self.content_sha256,
                byte_len: self.byte_len,
                line_count: self.line_count,
                created_at: self.document_created_at,
                updated_at: self.document_updated_at,
            },
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct GrepCandidateRow {
    pub(super) node_id: Uuid,
    pub(super) path_cache: String,
    pub(super) content_md: String,
}

impl GrepCandidateRow {
    pub(super) fn into_candidate(self) -> GrepCandidate {
        GrepCandidate {
            node_id: self.node_id,
            path: self.path_cache,
            content_md: self.content_md,
        }
    }
}
