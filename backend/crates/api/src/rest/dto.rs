//! Shared REST data-transfer objects and the mappers from service/model types.
//!
//! These mirror the exact JSON shapes in `docs/spec/rest/README.md`
//! (AccountRef, Workspace output, Node output, Document output). The api layer
//! owns these so the `model`/`service` types stay transport-free; mapping here is
//! thin (no domain logic).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_model::{AccountRef as ModelAccountRef, NodeKind};
use notegate_service::files::NodeView;
use notegate_service::workspaces::WorkspaceView;

/// A lightweight account reference: `{id, kind, display_name}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AccountRef {
    pub id: Uuid,
    pub kind: String,
    pub display_name: String,
}

impl From<&ModelAccountRef> for AccountRef {
    fn from(value: &ModelAccountRef) -> Self {
        Self {
            id: value.id,
            kind: value.kind.as_str().to_owned(),
            display_name: value.display_name.clone(),
        }
    }
}

impl AccountRef {
    /// A placeholder ref for an account id that could not be resolved (e.g. a
    /// hard-deleted account still referenced by an attribution column). Keeps the
    /// shape stable rather than failing the whole response.
    pub fn unknown(id: Uuid) -> Self {
        Self {
            id,
            kind: "user".to_owned(),
            display_name: String::new(),
        }
    }

    /// Resolve an id against a batch-loaded account map, falling back to a
    /// placeholder when absent.
    pub fn resolve(id: Uuid, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        refs.get(&id)
            .map(AccountRef::from)
            .unwrap_or_else(|| AccountRef::unknown(id))
    }
}

/// Workspace output: metadata, caller role, and derived `root_node_id`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkspaceOut {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub root_node_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&WorkspaceView> for WorkspaceOut {
    fn from(view: &WorkspaceView) -> Self {
        Self {
            id: view.workspace.id,
            name: view.workspace.name.clone(),
            role: view.role.as_str().to_owned(),
            root_node_id: view.root_node_id,
            created_at: view.workspace.created_at,
            updated_at: view.workspace.updated_at,
        }
    }
}

/// Node output: tree metadata, derived `path`, and attribution refs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeOut {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub sort_order: i32,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    pub created_by: AccountRef,
    pub updated_by: AccountRef,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl NodeOut {
    /// Map a [`NodeView`] to output, resolving attribution from a batch-loaded
    /// account map.
    pub fn from_view(view: &NodeView, refs: &HashMap<Uuid, ModelAccountRef>) -> Self {
        let node = &view.node;
        Self {
            id: node.id,
            workspace_id: node.workspace_id,
            parent_id: node.parent_id,
            name: node.name.clone(),
            kind: node.kind.as_str().to_owned(),
            path: view.path.clone(),
            sort_order: node.sort_order,
            has_children: view.has_children,
            content_sha256: view
                .document
                .as_ref()
                .map(|document| document.content_sha256.clone()),
            byte_len: view.document.as_ref().map(|document| document.byte_len),
            line_count: view.document.as_ref().map(|document| document.line_count),
            created_by: AccountRef::resolve(node.created_by, refs),
            updated_by: AccountRef::resolve(node.updated_by, refs),
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

/// The condensed node reference embedded in `children` and `document` responses
/// (`{id, path}` plus kind where the spec shows it).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeRef {
    pub id: Uuid,
    pub path: String,
    pub kind: String,
}

impl From<&NodeView> for NodeRef {
    fn from(view: &NodeView) -> Self {
        Self {
            id: view.node.id,
            path: view.path.clone(),
            kind: view.node.kind.as_str().to_owned(),
        }
    }
}

/// Collect the distinct `created_by`/`updated_by` account ids referenced by a set
/// of node views, for a single batched [`AccountRef`] resolution.
pub fn attribution_ids<'a>(views: impl IntoIterator<Item = &'a NodeView>) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for view in views {
        for id in [view.node.created_by, view.node.updated_by] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Parse a `kind` query/body string into a [`NodeKind`], rejecting unknowns.
pub fn parse_kind(value: &str) -> Result<NodeKind, crate::error::ApiError> {
    NodeKind::parse(value)
        .ok_or_else(|| crate::error::ApiError::invalid_field("kind must be 'folder' or 'document'"))
}
