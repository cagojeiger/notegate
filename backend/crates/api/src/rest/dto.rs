//! Shared REST data-transfer objects and the mappers from service/model types.
//!
//! These mirror the exact JSON shapes in `docs/spec/rest-api.md` (Page,
//! AccountRef, Workspace output, Node output, Document output). The api layer
//! owns these so the `model`/`service` types stay transport-free; mapping here is
//! thin (no domain logic).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_model::{AccountRef as ModelAccountRef, NodeKind, Role};
use notegate_service::files::NodeView;
use notegate_service::workspaces::WorkspaceView;

/// Pagination metadata returned by every list/search response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Page {
    pub limit: i64,
    pub returned: i64,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

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

/// Clamp a requested page limit to `1..=max`, defaulting to `default` when the
/// client did not supply one. A non-positive limit clamps to `1`.
pub fn clamp_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    match limit {
        None => default,
        Some(value) if value < 1 => 1,
        Some(value) => value.min(max),
    }
}

/// Keyset-paginate a fully-materialized, stably-ordered slice by item id.
///
/// The service layer returns these small bounded lists (workspaces ≤ owner quota,
/// access ≤ 20, agents ≤ 50) already ordered; this slices a window after the
/// cursor id and reports `has_more` plus the next cursor (the last item's id,
/// base64-encoded). Returns `Err` only when the cursor fails to decode (`400`).
pub fn paginate_by_id<'a, T>(
    items: &'a [T],
    id_of: impl Fn(&T) -> Uuid,
    limit: i64,
    cursor: Option<&str>,
) -> Result<(Vec<&'a T>, Page), crate::error::ApiError> {
    let start = match cursor {
        None => 0,
        Some(raw) => {
            let after: Uuid = notegate_service::cursor::decode(raw)
                .map_err(|_error| crate::error::ApiError::invalid_field("invalid cursor"))?;
            items
                .iter()
                .position(|item| id_of(item) == after)
                .map(|index| index + 1)
                .unwrap_or(items.len())
        }
    };

    let window: Vec<&T> = items.iter().skip(start).take(limit as usize).collect();
    let has_more = start + window.len() < items.len();
    let next_cursor = if has_more {
        window
            .last()
            .map(|item| notegate_service::cursor::encode(&id_of(item)))
            .transpose()
            .map_err(|_error| crate::error::ApiError::internal("failed to encode cursor"))?
    } else {
        None
    };

    let page = Page {
        limit,
        returned: window.len() as i64,
        has_more,
        next_cursor,
    };
    Ok((window, page))
}

/// Parse a `kind` query/body string into a [`NodeKind`], rejecting unknowns.
pub fn parse_kind(value: &str) -> Result<NodeKind, crate::error::ApiError> {
    NodeKind::parse(value)
        .ok_or_else(|| crate::error::ApiError::invalid_field("kind must be 'folder' or 'document'"))
}

/// Parse a `role` body string into a [`Role`], rejecting unknowns.
pub fn parse_role(value: &str) -> Result<Role, crate::error::ApiError> {
    Role::parse(value).ok_or_else(|| {
        crate::error::ApiError::invalid_field("role must be 'viewer', 'editor', or 'owner'")
    })
}
