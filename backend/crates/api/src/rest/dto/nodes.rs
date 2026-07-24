use std::collections::HashMap;

use chrono::{DateTime, Utc};
use notegate_model::{AccountRef as ModelAccountRef, FileEncryptionMode, NodeKind};
use notegate_service::files::NodeView;
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AccountRef;
use crate::file_preview::{is_preview_size_allowed, is_previewable_media_type};

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
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_available: Option<bool>,
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
            content_sha256: view.text.as_ref().map(|text| text.content_sha256.clone()),
            byte_len: view
                .text
                .as_ref()
                .map(|text| text.byte_len)
                .or_else(|| view.file.as_ref().map(|file| file.byte_len)),
            line_count: view.text.as_ref().map(|text| text.line_count),
            media_type: view.file.as_ref().map(|file| file.media_type.clone()),
            detected_media_type: view
                .file
                .as_ref()
                .and_then(|file| file.detected_media_type.clone()),
            preview_available: view.file.as_ref().and_then(|file| {
                if file.encryption_mode != FileEncryptionMode::None
                    || !is_preview_size_allowed(file.byte_len)
                {
                    return Some(false);
                }
                file.detected_media_type
                    .as_deref()
                    .map(is_previewable_media_type)
            }),
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
