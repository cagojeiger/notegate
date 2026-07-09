//! Shared REST data-transfer objects and the mappers from service/model types.
//!
//! These mirror the exact JSON shapes in `docs/spec/rest/README.md`
//! (AccountRef, Space output, Node output, Text output). The api layer
//! owns these so the `model`/`service` types stay transport-free; mapping here is
//! thin (no domain logic).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_model::{AccountRef as ModelAccountRef, ApiKey, AuditEvent, NodeKind};
use notegate_service::files::{FileChangeEvent, NodeView};
use notegate_service::spaces::SpaceView;

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

/// API-key creation request shared by user and agent key endpoints.
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateApiKeyBody {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
}

/// API-key metadata returned by key list endpoints. The plaintext token is never
/// included here.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiKeyMetadataOut {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<&ApiKey> for ApiKeyMetadataOut {
    fn from(key: &ApiKey) -> Self {
        Self {
            id: key.id,
            account_id: key.account_id,
            name: key.name.clone(),
            scopes: key.scopes.clone(),
            expires_at: key.expires_at,
            created_at: key.created_at,
            revoked_at: key.revoked_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ApiKeyMetadataListResponse {
    pub keys: Vec<ApiKeyMetadataOut>,
    pub page: crate::page::Page,
}

/// Audit event history entry returned by `GET /api/v1/me/audit-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub actor_account_id: Option<Uuid>,
    pub source: String,
    pub op_type: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub metadata: Value,
}

impl From<&AuditEvent> for AuditEventOut {
    fn from(event: &AuditEvent) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            actor_account_id: event.actor_account_id,
            source: event.source.clone(),
            op_type: event.op_type.clone(),
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id,
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditEventListResponse {
    pub events: Vec<AuditEventOut>,
    pub page: crate::page::Page,
}

/// File change event history entry returned by `GET /api/v1/spaces/{space_id}/file-change-events`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventOut {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub space_id: Uuid,
    pub node_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub op_type: String,
    pub metadata: Value,
}

impl From<&FileChangeEvent> for FileChangeEventOut {
    fn from(event: &FileChangeEvent) -> Self {
        Self {
            id: event.id,
            created_at: event.created_at,
            space_id: event.space_id,
            node_id: event.node_id,
            actor_account_id: event.actor_account_id,
            op_type: event.op_type.clone(),
            metadata: event.metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileChangeEventListResponse {
    pub events: Vec<FileChangeEventOut>,
    pub page: crate::page::Page,
}

/// Space output: metadata, caller permission, and derived `root_node_id`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpaceOut {
    pub id: Uuid,
    pub name: String,
    pub sort_order: i32,
    pub permission: String,
    pub root_node_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&SpaceView> for SpaceOut {
    fn from(view: &SpaceView) -> Self {
        Self {
            id: view.space.id,
            name: view.space.name.clone(),
            sort_order: view.space.sort_order,
            permission: view.permission.as_str().to_owned(),
            root_node_id: view.root_node_id,
            created_at: view.space.created_at,
            updated_at: view.space.updated_at,
        }
    }
}

/// Node output: tree metadata, derived `path`, and attribution refs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeOut {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub sort_order: i32,
    pub metadata: Value,
    pub has_children: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_metadata: Option<Value>,
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
            space_id: node.space_id,
            parent_id: node.parent_id,
            name: node.name.clone(),
            kind: node.kind.as_str().to_owned(),
            path: view.path.clone(),
            sort_order: node.sort_order,
            metadata: node.metadata.clone(),
            has_children: view.has_children,
            content_sha256: view
                .text
                .as_ref()
                .map(|text| text.content_sha256.clone())
                .or_else(|| view.file.as_ref().map(|file| file.content_sha256.clone())),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            storage_kind: view
                .file
                .as_ref()
                .map(|file| file.storage_kind.as_str().to_owned()),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            original_filename: view
                .file
                .as_ref()
                .and_then(|file| file.original_filename.clone()),
            encryption_mode: view
                .file
                .as_ref()
                .map(|file| file.encryption_mode.as_str().to_owned()),
            encryption_metadata: view
                .file
                .as_ref()
                .and_then(|file| file.encryption_metadata.clone()),
            created_by: AccountRef::resolve(node.created_by_account_id, refs),
            updated_by: AccountRef::resolve(node.updated_by_account_id, refs),
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

/// The condensed node reference embedded in `children` and `text` responses
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
        for id in [
            view.node.created_by_account_id,
            view.node.updated_by_account_id,
        ] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Parse a `kind` query/body string into a [`NodeKind`], rejecting unknowns.
pub fn parse_kind(value: &str) -> Result<NodeKind, crate::error::ApiError> {
    NodeKind::parse(value).ok_or_else(|| {
        crate::error::ApiError::invalid_field("kind must be 'folder', 'text', or 'file'")
    })
}
